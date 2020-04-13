use crate::horust::bus::BusConnector;
use crate::horust::error::Result;
use crate::horust::formats::{
    Event, ExitStatus, FailureStrategy, RestartStrategy, Service, ServiceHandler, ServiceName,
    ServiceStatus,
};
use crate::horust::{healthcheck, signal_handling};
use nix::sys::signal::{self, Signal};
use nix::unistd::{fork, getppid, ForkResult};
use nix::unistd::{getpid, Pid};
use shlex;
use std::ffi::{CStr, CString};
use std::fmt::Debug;
use std::ops::{Add, Mul};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Runtime {
    is_shutting_down: bool,
    repo: Repo,
}

#[derive(Debug, Clone)]
struct Repo {
    // TODO: make it a map ServiceName: ServiceHandler
    pub services: Vec<ServiceHandler>,
    pub(crate) bus: BusConnector,
}

impl Repo {
    fn new<T: Into<ServiceHandler>>(bus: BusConnector, services: Vec<T>) -> Self {
        let services = services.into_iter().map(Into::into).collect();
        Self { bus, services }
    }

    /// Returns true, if the repository is in a state for which fuhrer state transitions can be triggered
    /// only by external events.
    fn should_block(&self) -> bool {
        let triggering_states = vec![
            ServiceStatus::Running,
            ServiceStatus::Finished,
            ServiceStatus::FinishedFailed,
        ];

        self.services
            .iter()
            .all(|sh| triggering_states.contains(&sh.status))
    }

    // Non blocking
    fn get_events(&mut self) -> Vec<Event> {
        self.bus.try_get_events()
    }

    /// Blocking
    fn get_events_blocking(&mut self) -> Vec<Event> {
        vec![self.bus.get_events_blocking()]
    }

    /// Blocking
    fn get_n_events_blocking(&mut self, quantity: usize) -> Vec<Event> {
        self.bus.get_n_events_blocking(quantity)
    }

    pub fn all_finished(&self) -> bool {
        self.services
            .iter()
            .all(|sh| sh.is_finished() || sh.is_finished_failed())
    }

    pub fn get_mut_service(&mut self, service_name: &ServiceName) -> &mut ServiceHandler {
        self.services
            .iter_mut()
            .filter(|sh| sh.name() == service_name)
            .last()
            .unwrap()
    }
    /// Get all the services that have specifed "start-after = [`service_name`]" in their config
    fn get_dependents(&self, service_name: &ServiceName) -> Vec<ServiceName> {
        self.services
            .iter()
            .filter(|sh| sh.service().start_after.contains(service_name))
            .map(|sh| sh.name())
            .cloned()
            .collect()
    }

    fn get_die_if_failed(&self, service_name: &ServiceName) -> Vec<&ServiceName> {
        self.services
            .iter()
            .filter(|sh| {
                sh.service()
                    .termination
                    .die_if_failed
                    .contains(service_name)
            })
            .map(|sh| sh.name())
            .collect()
    }

    fn send_ev(&mut self, ev: Event) {
        self.bus.send_event(ev)
    }

    fn is_service_runnable(&self, sh: &ServiceHandler) -> bool {
        if !sh.is_initial() {
            return false;
        }
        let is_started = |service_name: &ServiceName| {
            self.services.iter().any(|service| {
                service.name() == service_name && (service.is_running() || service.is_finished())
            })
        };
        sh.start_after().iter().all(is_started)
    }
}

// Spawns and runs this component in a new thread.
pub fn spawn(bus: BusConnector, services: Vec<Service>) -> std::thread::JoinHandle<ExitStatus> {
    thread::spawn(move || Runtime::new(bus, services).run())
}

impl Runtime {
    fn new(bus: BusConnector, services: Vec<Service>) -> Self {
        let repo = Repo::new(bus, services);
        Self {
            repo,
            is_shutting_down: false,
        }
    }

    // Apply side effects
    fn apply_event(&mut self, ev: Event) {
        match ev {
            Event::StatusChanged(service_name, status) => {
                let service_handler = self.repo.get_mut_service(&service_name);
                match status {
                    ServiceStatus::ToBeKilled => {
                        if service_handler.status == ServiceStatus::Initial {
                            service_handler.status = ServiceStatus::Finished;
                        } else if vec![
                            ServiceStatus::Running,
                            ServiceStatus::Starting,
                            ServiceStatus::ToBeRun,
                        ]
                        .contains(&service_handler.status)
                        {
                            service_handler.status = ServiceStatus::ToBeKilled;
                            service_handler.shutting_down_start = Some(Instant::now());
                            kill(
                                service_handler,
                                service_handler.service().termination.signal.as_signal(),
                            );
                        } else {
                            error!(
                                "Service ToBeKilled was in status: {}",
                                service_handler.status
                            );
                        }
                    }
                    ServiceStatus::ToBeRun => {
                        if service_handler.status == ServiceStatus::Initial {
                            service_handler.status = ServiceStatus::ToBeRun;
                            healthcheck::prepare_service(&service_handler.service().healthiness)
                                .unwrap();
                            let backoff = service_handler
                                .service()
                                .restart
                                .backoff
                                .mul(service_handler.restart_attempts.clone());
                            run_spawning_thread(
                                service_handler.service().clone(),
                                backoff,
                                self.repo.clone(),
                            );
                        }
                    }
                    ServiceStatus::Running => {
                        if service_handler.status == ServiceStatus::Starting {
                            service_handler.status = ServiceStatus::Running;
                            service_handler.restart_attempts = 0;
                        }
                    }
                    ServiceStatus::Starting => {
                        if service_handler.status != ServiceStatus::InKilling {
                            service_handler.status = ServiceStatus::Starting;
                            service_handler.restart_attempts = 0;
                        }
                    }
                    unhandled_status => {
                        debug!(
                            "Unhandled status, setting: {}, {}",
                            service_name, unhandled_status
                        );
                        service_handler.status = unhandled_status;
                    }
                }
            }
            Event::ServiceExited(service_name, exit_code) => {
                let service_handler = self.repo.get_mut_service(&service_name);
                service_handler.shutting_down_start = None;
                service_handler.pid = None;

                let has_failed = !service_handler
                    .service()
                    .failure
                    .successful_exit_code
                    .contains(&exit_code);
                if has_failed {
                    error!(
                        "Service: {} has failed, exit code: {}",
                        service_handler.name(),
                        exit_code
                    );

                    // If it has failed too quickly, increase service_handler's restart attempts
                    // and check if it has more attempts left.
                    if vec![ServiceStatus::Starting, ServiceStatus::Initial]
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
            Event::ForceKill(service_name) => {
                let service_handler = self.repo.get_mut_service(&service_name);
                kill(&service_handler, Signal::SIGKILL);
                service_handler.status = ServiceStatus::FinishedFailed;
            }
            Event::PidChanged(service_name, pid) => {
                let service_handler = self.repo.get_mut_service(&service_name);
                service_handler.pid = Some(pid);
            }
            Event::ShuttingDownInitiated => self.is_shutting_down = true,
            ev => {
                trace!("ignoring: {:?}", ev);
            }
        }
    }

    /// Compute next state for each sh
    pub fn next(&self, service_handler: &ServiceHandler) -> Vec<Event> {
        if self.repo.is_service_runnable(&service_handler) {
            if self.is_shutting_down {
                vec![Event::new_status_changed(
                    service_handler.name(),
                    ServiceStatus::Finished,
                )]
            } else {
                vec![Event::new_status_changed(
                    service_handler.name(),
                    ServiceStatus::ToBeRun,
                )]
            }
        } else {
            match service_handler.status {
                ServiceStatus::Initial => {
                    if self.is_shutting_down {
                        vec![Event::new_status_changed(
                            service_handler.name(),
                            ServiceStatus::Finished,
                        )]
                    } else {
                        vec![]
                    }
                }
                ServiceStatus::Success => {
                    let service_ev = handle_restart_strategy(service_handler, false);
                    vec![service_ev]
                }
                ServiceStatus::Failed => {
                    let attempts_are_over = service_handler.restart_attempts
                        > service_handler.service().restart.attempts;

                    let mut failure_evs = handle_failure_strategy(
                        self.repo.get_dependents(service_handler.name().into()),
                        service_handler,
                    );
                    let other_services_termination = self
                        .repo
                        .get_die_if_failed(service_handler.name())
                        .into_iter()
                        .map(|sh_name| {
                            Event::new_status_changed(&sh_name, ServiceStatus::ToBeKilled)
                        });

                    let service_ev = if !attempts_are_over {
                        Event::new_status_changed(
                            service_handler.name(),
                            ServiceStatus::FinishedFailed,
                        )
                    } else {
                        handle_restart_strategy(service_handler, true)
                    };

                    failure_evs.push(service_ev);
                    failure_evs.extend(other_services_termination);
                    failure_evs
                }
                ServiceStatus::InKilling => {
                    if should_force_kill(service_handler) {
                        vec![Event::new_force_kill(service_handler.name())]
                    } else {
                        vec![]
                    }
                }
                ServiceStatus::Running | ServiceStatus::Starting => {
                    if self.is_shutting_down {
                        vec![Event::new_status_changed(
                            service_handler.name(),
                            ServiceStatus::ToBeKilled,
                        )]
                    } else {
                        vec![]
                    }
                }
                ServiceStatus::ToBeKilled => {
                    // Change to service in killing event.
                    vec![Event::new_status_changed(
                        service_handler.name(),
                        ServiceStatus::InKilling,
                    )]
                }
                _ => vec![],
            }
        }
    }

    /// Blocking call. Tries to move state machines forward
    pub fn run(mut self) -> ExitStatus {
        let mut has_emit_ev = 0;
        loop {
            // Ingest updates
            let events = if has_emit_ev > 0 {
                self.repo.get_n_events_blocking(has_emit_ev)
            } else {
                self.repo.get_events()
            };

            debug!("Applying events.. {:?}", events);
            //debug!("Service status: {:?}", self.repo.services);
            if signal_handling::is_sigterm_received() && !self.is_shutting_down {
                self.repo.send_ev(Event::ShuttingDownInitiated);
            }

            events.into_iter().for_each(|ev| self.apply_event(ev));

            let events: Vec<Event> = self
                .repo
                .services
                .iter()
                .map(|sh| self.next(sh))
                .flatten()
                .collect();
            debug!("Going to emit events: {:?}", events);
            has_emit_ev = events.len();
            events.into_iter().for_each(|ev| self.repo.send_ev(ev));

            // TODO: apply some clever check and exit if no service will never be started again.
            if self.repo.all_finished() {
                debug!("All services have finished, exiting...");
                break;
            } else {
            }
            std::thread::sleep(Duration::from_millis(300));
        }

        let res = if self.repo.services.iter().any(|sh| sh.is_finished_failed()) {
            ExitStatus::SomeServiceFailed
        } else {
            ExitStatus::Successful
        };

        if !self.is_shutting_down {
            self.repo.send_ev(Event::ShuttingDownInitiated);
        }
        self.repo
            .send_ev(Event::Exiting("Runtime".into(), ExitStatus::Successful));
        return res;
    }
}

fn should_force_kill(service_handler: &ServiceHandler) -> bool {
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
        error!("There is no shutting down elapsed secs!!");
        false
    }
}

fn handle_restart_strategy(service_handler: &ServiceHandler, is_failed: bool) -> Event {
    let new_status = |status| Event::new_status_changed(service_handler.name(), status);
    let ev = match service_handler.service().restart.strategy {
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
fn handle_failure_strategy(deps: Vec<ServiceName>, failed_sh: &ServiceHandler) -> Vec<Event> {
    match failed_sh.service().failure.strategy {
        FailureStrategy::Shutdown => vec![Event::ShuttingDownInitiated],
        FailureStrategy::KillDependents => {
            debug!("Failed service has kill-dependents strategy, going to mark them all..");
            deps.iter()
                .map(|sh| Event::new_status_changed(sh, ServiceStatus::ToBeKilled))
                .collect()
        }
        FailureStrategy::Ignore => vec![],
    }
}

fn kill(sh: &ServiceHandler, signal: Signal) {
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
        error!("Missing pid to kill but process was in running state.");
    }
}

/// Run another thread that will wait for the start delay, and handle the fork / exec.
fn run_spawning_thread(service: Service, backoff: Duration, mut repo: Repo) {
    std::thread::spawn(move || {
        // todo: we should wake up every second, in case someone wants to kill this process.
        std::thread::sleep(service.start_delay.add(backoff));
        let evs = match spawn_process(&service) {
            Ok(pid) => {
                debug!("Setting pid:{} for service: {}", pid, service.name);
                vec![
                    Event::new_pid_changed(service.name.clone(), pid),
                    Event::new_status_changed(&service.name, ServiceStatus::Starting),
                ]
            }
            Err(error) => {
                error!("Failed spawning the process: {}", error);
                vec![Event::new_status_changed(
                    &service.name,
                    ServiceStatus::Failed,
                )]
            }
        };
        evs.into_iter().for_each(|ev| repo.send_ev(ev));
    });
}

/// Fork the process
fn spawn_process(service: &Service) -> Result<Pid> {
    match fork() {
        Ok(ForkResult::Child) => {
            debug!("Child PID: {}, PPID: {}.", getpid(), getppid());
            exec_service(service);
            unreachable!()
        }
        Ok(ForkResult::Parent { child, .. }) => {
            debug!("Spawned child with PID {}.", child);
            Ok(child)
        }
        Err(err) => Err(Into::into(err)),
    }
}

fn exec_service(service: &Service) {
    let cwd = service.working_directory.clone();
    debug!("Set cwd: {:?}, ", cwd);

    std::env::set_current_dir(cwd).expect("Set cwd");
    nix::unistd::setsid().expect("Set sid");
    nix::unistd::setuid(service.user.get_uid()).expect("setuid");
    let chunks: Vec<String> = shlex::split(service.command.as_ref()).unwrap();
    let program_name = CString::new(chunks.get(0).unwrap().as_str()).unwrap();
    let to_cstring = |s: Vec<String>| {
        s.into_iter()
            .map(|arg| CString::new(arg).map_err(Into::into))
            .collect::<Result<Vec<_>>>()
            .unwrap()
    };
    let arg_cstrings = to_cstring(chunks);
    let arg_cptr: Vec<&CStr> = arg_cstrings.iter().map(|c| c.as_c_str()).collect();

    let env_cstrings = to_cstring(service.get_environment());
    let env_cptr: Vec<&CStr> = env_cstrings.iter().map(|c| c.as_c_str()).collect();

    //arg_cstrings.insert(0, program_name.clone());
    //TODO: if ENOENT exit status is 0
    nix::unistd::execvpe(program_name.as_ref(), arg_cptr.as_ref(), env_cptr.as_ref())
        .expect("Execvpe() failed: ");
}

#[cfg(test)]
mod test {
    use crate::horust::formats::{FailureStrategy, Service, ServiceHandler};
    use crate::horust::runtime::{handle_failure_strategy, should_force_kill};
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
        assert!(!should_force_kill(&sh));
        sh.shutting_down_start = Some(sh.shutting_down_start.unwrap().sub(Duration::from_secs(20)));
        assert!(should_force_kill(&sh))
    }

    #[test]
    fn test_handle_failed_service() {
        let mut service = Service::from_name("b");
        let evs = handle_failure_strategy(vec!["a".into()], &service.clone().into());
        println!("evs: {:?}", evs);
        assert!(evs.is_empty());

        service.failure.strategy = FailureStrategy::KillDependents;
        let evs = handle_failure_strategy(vec!["a".into()], &service.clone().into());
        println!("evs: {:?}", evs);
        assert_eq!(evs.len(), 1);

        service.failure.strategy = FailureStrategy::Shutdown;
        let evs = handle_failure_strategy(vec!["a".into()], &service.into());
        println!("evs: {:?}", evs);
        assert_eq!(evs.len(), 1);
    }
}
