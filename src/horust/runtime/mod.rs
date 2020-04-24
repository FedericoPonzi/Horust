use crate::horust::bus::BusConnector;
use crate::horust::formats::{
    Event, ExitStatus, FailureStrategy, HealthinessStatus, RestartStrategy, Service, ServiceName,
    ServiceStatus,
};
use crate::horust::{healthcheck, signal_handling};
use nix::sys::signal::{self, Signal};
use std::fmt::Debug;
use std::ops::Mul;
use std::thread;
use std::time::{Duration, Instant};

mod process_spawner;
mod service_handler;
use service_handler::ServiceHandler;
mod repo;
use repo::Repo;

#[derive(Debug)]
pub struct Runtime {
    is_shutting_down: bool,
    repo: Repo,
}

// Spawns and runs this component in a new thread.
pub fn spawn(
    bus: BusConnector<Event>,
    services: Vec<Service>,
) -> std::thread::JoinHandle<ExitStatus> {
    thread::spawn(move || Runtime::new(bus, services).run())
}

impl Runtime {
    fn new(bus: BusConnector<Event>, services: Vec<Service>) -> Self {
        let repo = Repo::new(bus, services);
        Self {
            repo,
            is_shutting_down: false,
        }
    }

    // Apply side effects
    fn apply_event(&mut self, ev: Event) {
        match ev {
            Event::StatusChanged(service_name, new_status) => {
                let mut service_handler = self.repo.get_mut_sh(&service_name);
                debug!(
                    " service: {},  status: {}, new status: {}",
                    service_handler.name(),
                    service_handler.status,
                    new_status
                );
                handle_status_changed_event(service_name, new_status, &mut service_handler);
            }
            Event::ServiceExited(service_name, exit_code) => {
                // TODO: this handler is not super nice because the states
                // Success / Failed are set here, thus not available on the outside.
                // I'm not sure if this is a problem, because one could handle this event,
                // But still it's not super nice. Needs some more thinking.
                let service_handler = self.repo.get_mut_sh(&service_name);
                service_handler.shutting_down_start = None;
                service_handler.pid = None;

                let has_failed = !service_handler
                    .service()
                    .failure
                    .successful_exit_code
                    .contains(&exit_code);
                let healthcheck_failed = service_handler.healthiness_checks_failed > 0
                    && service_handler.status == ServiceStatus::Running;
                if has_failed || healthcheck_failed {
                    warn!(
                        "Service: {} has failed, exit code: {}, healthchecks: {}",
                        service_handler.name(),
                        exit_code,
                        healthcheck_failed
                    );

                    // If it has failed too quickly, increase service_handler's restart attempts
                    // and check if it has more attempts left.
                    if vec![ServiceStatus::Started, ServiceStatus::Initial]
                        .contains(&service_handler.status)
                    {
                        service_handler.restart_attempts += 1;
                        if service_handler.restart_attempts_are_over() {
                            service_handler.status = ServiceStatus::Failed;
                        } else {
                            service_handler.status = ServiceStatus::Initial;
                        }
                    } else {
                        // If wasn't starting, then it's just failed in a usual way:
                        service_handler.status = ServiceStatus::Failed;
                    }
                } else {
                    info!(
                        "Service: {} successfully exited with: {}.",
                        service_handler.name(),
                        exit_code
                    );
                    service_handler.status = ServiceStatus::Success;
                }
                debug!("New state for exited service: {:?}", service_handler.status);
            }
            Event::Run(service_name) if self.repo.get_sh(&service_name).is_initial() => {
                let service_handler = self.repo.get_mut_sh(&service_name);
                service_handler.status = ServiceStatus::Starting;
                healthcheck::prepare_service(&service_handler.service().healthiness)
                    .expect("Prepare healthcheck failed");
                let backoff = service_handler
                    .service()
                    .restart
                    .backoff
                    .mul(service_handler.restart_attempts);
                process_spawner::spawn_fork_exec_handler(
                    service_handler.service().clone(),
                    backoff,
                    self.repo.bus.clone(),
                );
            }
            Event::Kill(service_name) => {
                debug!("Received kill request");
                let service_handler = self.repo.get_mut_sh(&service_name);
                if service_handler.is_in_killing() {
                    service_handler.shutting_down_started();
                    kill(service_handler, None);
                } else {
                    debug!(
                        "Cannot send kill request, service was in: {}",
                        service_handler.status
                    );
                }
            }
            Event::ForceKill(service_name) if self.repo.get_sh(&service_name).is_in_killing() => {
                let service_handler = self.repo.get_mut_sh(&service_name);
                kill(&service_handler, Some(Signal::SIGKILL));
            }
            Event::PidChanged(service_name, pid) => {
                let service_handler = self.repo.get_mut_sh(&service_name);
                service_handler.pid = Some(pid);
                if service_handler.is_in_killing() {
                    // Ah! Gotcha!
                    service_handler.shutting_down_start = Some(Instant::now());
                    kill(service_handler, None)
                }
            }
            Event::HealthCheck(s_name, health) => {
                let sh = self.repo.get_mut_sh(&s_name);
                // Count the failed healthiness checks. The state change producer wll handle states
                // changes (if they're needed)
                if vec![ServiceStatus::Running, ServiceStatus::Started].contains(&sh.status) {
                    if let HealthinessStatus::Healthy = health {
                        sh.healthiness_checks_failed = 0;
                    } else {
                        sh.healthiness_checks_failed += 1;
                    }
                }
            }
            Event::ShuttingDownInitiated => self.is_shutting_down = true,
            ev => {
                trace!("ignoring: {:?}", ev);
            }
        }
    }

    /// Compute next state for each sh
    fn next(&self, service_handler: &ServiceHandler) -> Vec<Event> {
        let ev_status =
            |status: ServiceStatus| Event::new_status_changed(service_handler.name(), status);
        let vev_status = |status: ServiceStatus| vec![ev_status(status)];

        if self.is_shutting_down {
            // Handle the new state separately if we're shutting down.
            return match service_handler.status {
                ServiceStatus::Running | ServiceStatus::Started => vec![
                    ev_status(ServiceStatus::InKilling),
                    Event::Kill(service_handler.name().clone()),
                ],
                ServiceStatus::Success | ServiceStatus::Initial => {
                    vev_status(ServiceStatus::Finished)
                }
                ServiceStatus::Failed => vev_status(ServiceStatus::FinishedFailed),
                ServiceStatus::InKilling if should_force_kill(service_handler) => vec![
                    Event::new_force_kill(service_handler.name()),
                    Event::new_status_changed(service_handler.name(), ServiceStatus::Failed),
                ],
                _ => vec![],
            };
        }
        if self.repo.is_service_runnable(&service_handler) {
            return vec![Event::Run(service_handler.name().clone())];
        }

        match service_handler.status {
            ServiceStatus::Started if service_handler.healthiness_checks_failed == 0 => {
                vev_status(ServiceStatus::Running)
            }
            // If 2 healthcheks are failed, then kill the service. Maybe this should be parametrized
            ServiceStatus::Running if service_handler.healthiness_checks_failed > 2 => vec![
                ev_status(ServiceStatus::InKilling),
                Event::Kill(service_handler.name().clone()),
            ],
            ServiceStatus::Success => {
                vec![handle_restart_strategy(service_handler.service(), false)]
            }
            ServiceStatus::Failed => {
                let attempts_are_over =
                    service_handler.restart_attempts > service_handler.service().restart.attempts;

                let mut failure_evs = handle_failure_strategy(
                    self.repo.get_dependents(service_handler.name().into()),
                    service_handler.service(),
                );
                let other_services_termination = self
                    .repo
                    .get_die_if_failed(service_handler.name())
                    .into_iter()
                    .map(|sh_name| {
                        vec![
                            Event::new_status_changed(sh_name, ServiceStatus::InKilling),
                            Event::Kill(sh_name.clone()),
                        ]
                    })
                    .flatten();

                let service_ev = if !attempts_are_over {
                    ev_status(ServiceStatus::FinishedFailed)
                } else {
                    handle_restart_strategy(service_handler.service(), true)
                };

                failure_evs.push(service_ev);
                failure_evs.extend(other_services_termination);
                failure_evs
            }
            ServiceStatus::InKilling if should_force_kill(service_handler) => vec![
                Event::new_force_kill(service_handler.name()),
                Event::new_status_changed(service_handler.name(), ServiceStatus::Failed),
            ],

            _ => vec![],
        }
    }

    /// Blocking call. Tries to move state machines forward
    fn run(mut self) -> ExitStatus {
        let mut has_emit_ev = 0;
        while !self.repo.all_have_finished() {
            // Ingest updates
            let events = if has_emit_ev > 0 {
                self.repo.get_n_events_blocking(has_emit_ev)
            } else {
                self.repo.get_events()
            };

            debug!("Applying events.. {:?}", events);
            if signal_handling::is_sigterm_received() && !self.is_shutting_down {
                self.repo.send_ev(Event::ShuttingDownInitiated);
            }

            events.into_iter().for_each(|ev| self.apply_event(ev));

            let events: Vec<Event> = self
                .repo
                .services
                .iter()
                .map(|(_s_name, sh)| self.next(sh))
                .flatten()
                .collect();
            debug!("Going to emit events: {:?}", events);
            has_emit_ev = events.len();
            events.into_iter().for_each(|ev| self.repo.send_ev(ev));
            std::thread::sleep(Duration::from_millis(300));
        }

        debug!("All services have finished, exiting...");

        let res = if self.repo.any_finished_failed() {
            ExitStatus::SomeServiceFailed
        } else {
            ExitStatus::Successful
        };

        if !self.is_shutting_down {
            self.repo.send_ev(Event::ShuttingDownInitiated);
        }
        self.repo.send_ev(Event::new_exit_success("Runtime"));
        return res;
    }
}

// TODO: test
/// Handles the status changed event
fn handle_status_changed_event(
    service_name: ServiceName,
    new_status: ServiceStatus,
    service_handler: &mut ServiceHandler,
) {
    // A -> [B,C] means that transition to A is allowed only if service is in state B or C.
    let allowed_transitions = hashmap! {
        ServiceStatus::Initial        => vec![ServiceStatus::Success, ServiceStatus::Failed],
        ServiceStatus::Started        => vec![ServiceStatus::Starting],
        ServiceStatus::InKilling      => vec![ServiceStatus::Initial,
                                              ServiceStatus::Running,
                                              ServiceStatus::Started],
        ServiceStatus::Running        => vec![ServiceStatus::Started],
        ServiceStatus::FinishedFailed => vec![ServiceStatus::Failed, ServiceStatus::InKilling],
        ServiceStatus::Success        => vec![ServiceStatus::Starting,
                                              ServiceStatus::Running,
                                              ServiceStatus::InKilling],
        ServiceStatus::Failed         => vec![ServiceStatus::Starting,
                                              ServiceStatus::Running,
                                              ServiceStatus::InKilling],
        ServiceStatus::Finished       => vec![ServiceStatus::Success,
                                             ServiceStatus::Initial],
    };
    let allowed = allowed_transitions.get(&new_status).unwrap();
    if allowed.contains(&service_handler.status) {
        match new_status {
            ServiceStatus::Started if allowed.contains(&service_handler.status) => {
                service_handler.status = ServiceStatus::Started;
                service_handler.restart_attempts = 0;
            }
            ServiceStatus::Running if allowed.contains(&service_handler.status) => {
                service_handler.status = ServiceStatus::Running;
                service_handler.healthiness_checks_failed = 0;
            }
            ServiceStatus::InKilling if allowed.contains(&service_handler.status) => {
                debug!(
                    " service: {},  status: {}, new status: {}",
                    service_handler.name(),
                    service_handler.status,
                    new_status
                );
                if service_handler.status == ServiceStatus::Initial {
                    debug!("Was in initial state, settign to finished");
                    service_handler.status = ServiceStatus::Success;
                } else {
                    service_handler.status = ServiceStatus::InKilling;
                }
            }
            new_status => {
                service_handler.status = new_status;
            }
        }
    } else {
        debug!(
            "Tried to make an illegal transition: (current) {} -> {} (received) for service: {}",
            service_handler.status, new_status, service_name
        );
    }
}

/// Produce events based on the Restart Strategy of the service.
fn handle_restart_strategy(service: &Service, is_failed: bool) -> Event {
    let new_status = |status| Event::new_status_changed(&service.name, status);
    let ev = match service.restart.strategy {
        RestartStrategy::Never => {
            if is_failed {
                new_status(ServiceStatus::FinishedFailed)
            } else {
                new_status(ServiceStatus::Finished)
            }
        }
        RestartStrategy::OnFailure => {
            if is_failed {
                new_status(ServiceStatus::Initial)
            } else {
                new_status(ServiceStatus::Finished)
            }
        }
        RestartStrategy::Always => new_status(ServiceStatus::Initial),
    };
    debug!("Restart strategy applied, ev: {:?}", ev);
    ev
}

/// This is applied to both failed and FinishedFailed services.
fn handle_failure_strategy(deps: Vec<ServiceName>, failed_sh: &Service) -> Vec<Event> {
    match failed_sh.failure.strategy {
        FailureStrategy::Shutdown => vec![Event::ShuttingDownInitiated],
        FailureStrategy::KillDependents => {
            debug!("Failed service has kill-dependents strategy, going to mark them all..");
            deps.iter()
                .map(|sh| {
                    vec![
                        Event::new_status_changed(sh, ServiceStatus::InKilling),
                        Event::Kill(sh.clone()),
                    ]
                })
                .flatten()
                .collect()
        }
        FailureStrategy::Ignore => vec![],
    }
}

/// Check if we've waitied enough for the service to exit
fn should_force_kill(service_handler: &ServiceHandler) -> bool {
    if service_handler.pid.is_none() {
        // Since it was in the started state, it doesn't have a pid yet.
        // Let's give it the time to start and exit.
        return false;
    }
    if let Some(shutting_down_elapsed_secs) = service_handler.shutting_down_start.clone() {
        let shutting_down_elapsed_secs = shutting_down_elapsed_secs.elapsed().as_secs();
        debug!(
            "{}, should not force kill. Elapsed: {}, termination wait: {}",
            service_handler.name(),
            shutting_down_elapsed_secs,
            service_handler.service().termination.wait.clone().as_secs()
        );
        shutting_down_elapsed_secs > service_handler.service().termination.wait.clone().as_secs()
    } else {
        // this might happen, because InKilling state is emitted before the Kill event.
        // So maybe the runtime has received only the InKilling state change, but hasn't sent the
        // signal yet. So it should be fine.
        debug!("There is no shutting down elapsed secs.");
        false
    }
}

/// Kill wrapper, will send signal to sh and handles the result.
/// By default it will send the signal defined in the termination section of the service.
fn kill(sh: &ServiceHandler, signal: Option<Signal>) {
    let signal = signal.unwrap_or_else(|| sh.service().termination.signal.clone().into());
    debug!("Going to send {} signal to pid {:?}", signal, sh.pid());
    if let Some(pid) = sh.pid() {
        if let Err(error) = signal::kill(pid, signal) {
            match error.as_errno().expect("errno empty!") {
                nix::errno::Errno::ESRCH => (),
                _ => error!(
                    "Error killing the process: {}, service: {}, pid: {:?}",
                    error,
                    sh.name(),
                    pid,
                ),
            }
        }
    } else {
        warn!(
            "{}: Missing pid to kill but service was in {:?} state.",
            sh.name(),
            sh.status
        );
    }
}

#[cfg(test)]
mod test {
    use crate::horust::formats::{FailureStrategy, Service, ServiceStatus};
    use crate::horust::runtime::service_handler::ServiceHandler;
    use crate::horust::runtime::{handle_failure_strategy, should_force_kill};
    use crate::horust::Event;
    use nix::unistd::Pid;
    use std::ops::Sub;
    use std::time::Duration;

    #[test]
    fn test_should_force_kill() {
        let service = r#"command="notrelevant"
[termination]
wait = "10s"
"#;
        let service: Service = toml::from_str(service).unwrap();
        let mut sh: ServiceHandler = service.into();
        assert!(!should_force_kill(&sh));
        sh.shutting_down_started();
        sh.status = ServiceStatus::InKilling;
        assert!(!should_force_kill(&sh));
        let old_start = sh.shutting_down_start;
        let past_wait = Some(sh.shutting_down_start.unwrap().sub(Duration::from_secs(20)));
        sh.shutting_down_start = past_wait;
        assert!(!should_force_kill(&sh));
        sh.pid = Some(Pid::this());
        sh.shutting_down_start = old_start;
        assert!(!should_force_kill(&sh));
        sh.shutting_down_start = past_wait;
        assert!(should_force_kill(&sh));
    }

    #[test]
    fn test_handle_failed_service() {
        let mut service = Service::from_name("b");
        let evs = handle_failure_strategy(vec!["a".into()], &service.clone());
        assert!(evs.is_empty());

        service.failure.strategy = FailureStrategy::KillDependents;
        let evs = handle_failure_strategy(vec!["a".into()], &service.clone());
        let exp = vec![
            Event::new_status_changed(&"a".to_string(), ServiceStatus::InKilling),
            Event::Kill("a".into()),
        ];
        assert_eq!(evs, exp);

        service.failure.strategy = FailureStrategy::Shutdown;
        let evs = handle_failure_strategy(vec!["a".into()], &service.into());
        let exp = vec![Event::ShuttingDownInitiated];
        assert_eq!(evs, exp);
    }
}
