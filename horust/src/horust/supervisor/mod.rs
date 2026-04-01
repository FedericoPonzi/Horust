//! The supervisor is one of the biggest modules. It is responsible for supervising the services, and
//! keeping track of their current state.
//! It will also reap the dead processes

use std::fmt::Debug;
use std::ops::Mul;
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal;
use nix::unistd;

pub(crate) use process_spawner::find_program;
use repo::Repo;
use service_handler::ServiceHandler;
pub(crate) use signal_handling::init;

use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, ExitStatus, Service, ServiceStatus, ShuttingDown};
use crate::horust::healthcheck;

mod process_spawner;
mod reaper;
mod repo;
mod service_handler;
mod signal_handling;

#[cfg(test)]
mod test_utils {
    use crate::horust::bus::{Bus, BusConnector};
    use crate::horust::formats::{Event, Service, ServiceStatus};
    use crate::horust::supervisor::repo::Repo;
    use crate::horust::supervisor::service_handler::ServiceHandler;

    fn make_bus_connector() -> BusConnector<Event> {
        let bus: Bus<Event> = Bus::new();
        let connector = bus.join_bus();
        std::thread::spawn(move || bus.run());
        connector
    }

    pub fn make_handler(name: &str, status: ServiceStatus) -> ServiceHandler {
        let mut sh: ServiceHandler = Service::from_name(name).into();
        sh.status = status;
        sh
    }

    pub fn make_repo_from_services(services: Vec<Service>) -> Repo {
        Repo::new(make_bus_connector(), services)
    }

    pub fn make_repo(services_with_status: Vec<(&str, ServiceStatus)>) -> Repo {
        let svc_list: Vec<Service> = services_with_status
            .iter()
            .map(|(name, _)| Service::from_name(name))
            .collect();
        let mut repo = Repo::new(make_bus_connector(), svc_list);

        for (name, status) in &services_with_status {
            let sh = repo.services.get_mut(*name).unwrap();
            sh.status = status.clone();
        }
        repo
    }

    pub fn make_repo_with_start_after(services: Vec<(&str, ServiceStatus, Vec<&str>)>) -> Repo {
        let svc_list: Vec<Service> = services
            .iter()
            .map(|(name, _, deps)| {
                let mut svc = Service::from_name(name);
                svc.start_after = deps.iter().map(|d| d.to_string()).collect();
                svc
            })
            .collect();
        let mut repo = Repo::new(make_bus_connector(), svc_list);

        for (name, status, _) in &services {
            let sh = repo.services.get_mut(*name).unwrap();
            sh.status = status.clone();
        }
        repo
    }
}

/// How many pid reap per iteration of the reaper
const MAX_PROCESS_REAPS_ITERS: u32 = 20;

/// PID 1 is reserved for the init process.
const INIT_PID: unistd::Pid = unistd::Pid::from_raw(1);

// Spawns and runs this component in a new thread.
pub fn spawn(bus: BusConnector<Event>, services: Vec<Service>) -> thread::JoinHandle<ExitStatus> {
    thread::spawn(move || Supervisor::new(bus, services).run())
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum LifecycleStatus {
    Running,
    ShuttingDown(ShuttingDown),
}

#[derive(Debug)]
pub struct Supervisor {
    /// The system is shutting down, no more services will be spawned.
    status: LifecycleStatus,
    repo: Repo,
}

impl Supervisor {
    fn new(bus: BusConnector<Event>, services: Vec<Service>) -> Self {
        let repo = Repo::new(bus, services);
        Self {
            repo,
            status: LifecycleStatus::Running,
        }
    }

    /// Handle the events, returns Events (state changes) to be dispatched.
    fn handle_event(&mut self, ev: Event) -> Vec<Event> {
        match ev {
            Event::ServiceExited(service_name, exit_code) => {
                let pid = self.repo.get_sh(&service_name).pid.unwrap();
                self.repo.remove_pid(pid);
                let service_handler = self.repo.get_mut_sh(&service_name);
                service_handler.shutting_down_start = None;
                service_handler.pid = None;

                // If an explicit restart was requested, bypass the restart strategy
                // and force the service back to Initial.
                if service_handler.restart_pending {
                    service_handler.restart_pending = false;
                    service_handler.restart_attempts = 0;
                    service_handler.healthiness_checks_failed = None;
                    // Transition through Failed first (valid from InKilling)
                    let (new_sh, _) = service_handler.change_status(ServiceStatus::Failed);
                    self.repo.insert_sh_by_name(service_name.clone(), new_sh);
                    // Then transition Failed → Initial (valid per FSM)
                    let sh = self.repo.get_sh(&service_name);
                    let (new_sh, new_status) = sh.change_status(ServiceStatus::Initial);
                    self.repo.insert_sh_by_name(service_name.clone(), new_sh);
                    return vec![Event::StatusChanged(service_name, new_status)];
                }

                let has_failed = !service_handler
                    .service()
                    .failure
                    .successful_exit_code
                    .contains(&exit_code);

                // If it has failed too quickly, increase service_handler's restart attempts
                // and check if it has more attempts left.
                service_handler.restart_attempts += u32::from(
                    service_handler.has_some_failed_healthchecks()
                        && service_handler.is_early_state(),
                );

                let new_status = if has_failed
                    || (service_handler.status == ServiceStatus::Running
                        && service_handler.has_some_failed_healthchecks())
                {
                    warn!(
                        "Service: {} has failed, exit code: {}, healthchecks: {} ({:?})",
                        service_handler.name(),
                        exit_code,
                        service_handler.has_some_failed_healthchecks(),
                        service_handler.healthiness_checks_failed
                    );
                    ServiceStatus::Failed
                } else {
                    info!(
                        "Service: {} successfully exited with: {}.",
                        service_handler.name(),
                        exit_code
                    );
                    ServiceStatus::Success
                };
                let (new_sh, new_status) = service_handler.change_status(new_status);
                self.repo.insert_sh_by_name(service_name.clone(), new_sh);
                debug!(
                    "{}: new status for exited service: {:?}",
                    service_name, new_status
                );
                vec![Event::StatusChanged(service_name, new_status)]
            }
            Event::Run(service_name) if self.repo.get_sh(&service_name).is_initial() => {
                let service_handler = self.repo.get_mut_sh(&service_name);
                service_handler.status = ServiceStatus::Starting;
                let evs = vec![Event::StatusChanged(service_name, ServiceStatus::Starting)];

                let res = healthcheck::prepare_service(&service_handler.service().healthiness);
                if res.is_err() {
                    //TODO: maybe this is a bit too aggressive.
                    error!(
                        "Prepare healthiness checks failed for service: {}, shutting down...",
                        service_handler.name()
                    );
                    service_handler.status = ServiceStatus::FinishedFailed;
                    self.status = LifecycleStatus::ShuttingDown(ShuttingDown::Gracefully);
                    return vec![
                        Event::StatusUpdate(
                            service_handler.name().clone(),
                            ServiceStatus::FinishedFailed,
                        ),
                        Event::ShuttingDownInitiated(ShuttingDown::Gracefully),
                    ];
                }
                let backoff = service_handler
                    .service()
                    .restart
                    .backoff
                    .mul(service_handler.restart_attempts);
                process_spawner::spawn_fork_exec_handler(
                    service_handler.service().clone(),
                    backoff,
                    self.repo.bus.join_bus(),
                );
                evs
            }
            Event::SpawnFailed(s_name) => {
                let service_handler = self.repo.get_mut_sh(&s_name);
                service_handler.status = ServiceStatus::Failed;
                vec![Event::StatusUpdate(s_name, ServiceStatus::Failed)]
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
                vec![]
            }
            Event::ForceKill(service_name) if self.repo.get_sh(&service_name).is_in_killing() => {
                debug!("Going to forcekill {}", service_name);
                let service_handler = self.repo.get_mut_sh(&service_name);
                kill(service_handler, Some(signal::SIGKILL));
                service_handler.status = ServiceStatus::Failed;
                vec![Event::new_status_changed(
                    service_handler.name(),
                    ServiceStatus::Failed,
                )]
            }
            Event::PidChanged(service_name, pid) => {
                self.repo.add_pid(pid, service_name.clone());

                let service_handler = self.repo.get_mut_sh(&service_name);
                service_handler.pid = Some(pid);
                if service_handler.is_in_killing() {
                    // Ah! Gotcha!
                    service_handler.shutting_down_start = Some(Instant::now());
                    kill(service_handler, None)
                } else {
                    service_handler.status = ServiceStatus::Started;
                    return vec![Event::StatusChanged(service_name, ServiceStatus::Started)];
                }

                vec![]
            }
            Event::HealthCheck(s_name, health) => {
                let sh = self.repo.get_mut_sh(&s_name);
                // Count the failed healthiness checks. The state change producer will handle states
                // changes (if they're needed)
                sh.add_healthcheck_event(health);
                vec![]
            }
            Event::Restart(service_name) => {
                let sh = self.repo.get_mut_sh(&service_name);
                if sh.status == ServiceStatus::InKilling {
                    // Already being killed; just mark for restart after exit
                    sh.restart_pending = true;
                    vec![]
                } else if sh.is_alive_state() {
                    sh.restart_pending = true;
                    let (new_sh, new_status) = sh.change_status(ServiceStatus::InKilling);
                    self.repo.insert_sh_by_name(service_name.clone(), new_sh);
                    let sh = self.repo.get_mut_sh(&service_name);
                    sh.shutting_down_started();
                    kill(sh, None);
                    vec![Event::StatusChanged(service_name, new_status)]
                } else {
                    // Terminal state (Finished/FinishedFailed/Success/Failed): go to Initial
                    let (new_sh, new_status) = sh.change_status(ServiceStatus::Initial);
                    self.repo.insert_sh_by_name(service_name.clone(), new_sh);
                    vec![Event::StatusChanged(service_name, new_status)]
                }
            }
            Event::ShuttingDownInitiated(shutting_down) => {
                match shutting_down {
                    ShuttingDown::Gracefully => {
                        warn!("Gracefully stopping...");
                    }
                    ShuttingDown::Forcefully => {
                        warn!("Terminating all services...");
                    }
                }
                self.status = LifecycleStatus::ShuttingDown(shutting_down);
                signal_handling::clear_sigtem();
                vec![]
            }
            Event::StatusUpdate(service_name, new_status) => {
                let service_handler = self.repo.get_sh(&service_name);

                let (new_sh, new_status) = service_handler.change_status(new_status);
                if new_status != service_handler.status {
                    self.repo.insert_sh_by_name(service_name.clone(), new_sh);
                    // this is the only place where the new_status changed is emitted.
                    vec![Event::new_status_changed(&service_name, new_status)]
                } else {
                    debug!(
                        "Status Update event handler, new status {} == {} old status",
                        new_status, service_handler.status
                    );
                    vec![]
                }
            }
            ev => {
                trace!("ignoring: {:?}", ev);
                vec![]
            }
        }
    }

    /// One iteration of the supervisor loop: ingest events, handle them, produce next events.
    fn tick(&mut self) {
        // Ingest updates
        let received_events = self.repo.get_events();
        debug!("Applying events... {:?}", received_events);
        match (self.status, signal_handling::is_sigterm_received()) {
            (LifecycleStatus::Running, true) => {
                warn!("1. SIGTERM received");
                self.repo
                    .send_ev(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
            }
            (LifecycleStatus::ShuttingDown(ShuttingDown::Gracefully), true) => {
                warn!("2. SIGTERM received");
                self.repo
                    .send_ev(Event::ShuttingDownInitiated(ShuttingDown::Forcefully));
            }
            _ => {}
        }
        // Handling of the received events and commands:
        let produced_events = received_events
            .into_iter()
            .flat_map(|ev| self.handle_event(ev))
            .collect::<Vec<Event>>();
        debug!("Produced events: {:?}", produced_events);
        // Producing commands which will be applied in the next iteration
        let next_evs: Vec<Event> = self
            .repo
            .services
            .iter()
            .flat_map(|(_s_name, sh)| sh.next(&self.repo, self.status))
            .chain(reaper::run(&self.repo, MAX_PROCESS_REAPS_ITERS))
            .collect();
        debug!("Next evs: {:?}", next_evs);
        // Dispatch everything via the bus. Since the bus is run by another thread,
        // the next_evs might not arrive in the next batch, leading to possibly duplicated
        // commands.
        produced_events
            .into_iter()
            .chain(next_evs)
            .for_each(|ev| self.repo.send_ev(ev));

        thread::sleep(Duration::from_millis(300));
    }

    /// Blocking call.
    /// This function will run the services and reap dead pids.
    fn run(mut self) -> ExitStatus {
        while !self.repo.all_have_finished() {
            self.tick();
        }
        // Drain one more round of events to avoid the exit race:
        // a restart/start command may have arrived after all_have_finished() was true.
        self.tick();
        if !self.repo.all_have_finished() {
            // New work appeared (e.g. restart/start command), keep running.
            while !self.repo.all_have_finished() {
                self.tick();
            }
        }

        debug!("All services have finished");
        // If we're the init system, let's be sure that everything stops before exiting.
        // TODO: Test (probably via docker).
        if unistd::getpid() == INIT_PID {
            let all_processes = unistd::Pid::from_raw(-1);
            let _res = signal::kill(all_processes, signal::SIGTERM);
            thread::sleep(Duration::from_secs(3));
            let _res = signal::kill(all_processes, signal::SIGKILL);
        }

        self.repo
            .send_ev(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
        if self.repo.any_finished_failed() {
            ExitStatus::SomeServiceFailed
        } else {
            ExitStatus::Successful
        }
    }
}

/// A Kill wrapper which will send a signal to sh.
/// It will send the signal set out in the termination section of the service
fn kill(sh: &ServiceHandler, signal: Option<signal::Signal>) {
    let signal = signal.unwrap_or_else(|| sh.service().termination.signal.into());
    debug!("Going to send {} signal to pid {:?}", signal, sh.pid());
    if let Some(pid) = sh.pid() {
        if let Err(error) = signal::kill(pid, signal) {
            match error {
                // No process or process group can be found corresponding to that specified by pid
                // It has exited already, so it's fine.
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
mod supervisor_tests {
    use super::*;
    use crate::horust::bus::Bus;
    use crate::horust::formats::{Service, ServiceStatus};

    fn make_supervisor(services: Vec<(&str, ServiceStatus)>) -> Supervisor {
        let bus: Bus<Event> = Bus::new();
        let connector = bus.join_bus();
        std::thread::spawn(move || bus.run());
        let svc_list: Vec<Service> = services
            .iter()
            .map(|(name, _)| Service::from_name(name))
            .collect();
        let mut sup = Supervisor::new(connector, svc_list);
        for (name, status) in &services {
            let sh = sup.repo.get_mut_sh(name);
            sh.status = status.clone();
        }
        sup
    }

    /// Regression: restarting a service already in InKilling should set restart_pending
    /// without attempting an invalid FSM transition (InKilling → Initial is not allowed).
    #[test]
    fn test_restart_on_inkilling_service_sets_pending() {
        let mut sup = make_supervisor(vec![("svc", ServiceStatus::InKilling)]);
        let events = sup.handle_event(Event::Restart("svc".into()));
        // Should produce no status-change events (service is already being killed)
        assert!(events.is_empty(), "Expected no events, got: {:?}", events);
        // But restart_pending should be set
        let sh = sup.repo.get_sh("svc");
        assert!(
            sh.restart_pending,
            "restart_pending should be true for InKilling service"
        );
    }

    /// Regression: restarting a Running service should use change_status() to transition
    /// to InKilling (valid FSM transition), not directly assign the status field.
    #[test]
    fn test_restart_on_running_service_transitions_via_fsm() {
        let mut sup = make_supervisor(vec![("svc", ServiceStatus::Running)]);
        let events = sup.handle_event(Event::Restart("svc".into()));
        // Should produce a StatusChanged event to InKilling
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], Event::StatusChanged(name, ServiceStatus::InKilling) if name == "svc"),
            "Expected StatusChanged to InKilling, got: {:?}",
            events[0]
        );
        // Service should be InKilling with restart_pending
        let sh = sup.repo.get_sh("svc");
        assert_eq!(sh.status, ServiceStatus::InKilling);
        assert!(sh.restart_pending);
    }

    /// Regression: restarting a terminal-state service (e.g. Failed) should transition
    /// to Initial via change_status() (valid FSM: Failed → Initial).
    #[test]
    fn test_restart_on_failed_service_transitions_to_initial() {
        let mut sup = make_supervisor(vec![("svc", ServiceStatus::Failed)]);
        let events = sup.handle_event(Event::Restart("svc".into()));
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], Event::StatusChanged(name, ServiceStatus::Initial) if name == "svc"),
            "Expected StatusChanged to Initial, got: {:?}",
            events[0]
        );
        let sh = sup.repo.get_sh("svc");
        assert_eq!(sh.status, ServiceStatus::Initial);
    }

    /// Regression: when restart_pending is set and service exits, it should transition
    /// back to Initial via FSM (InKilling → Failed → Initial), not via direct assignment.
    #[test]
    fn test_service_exited_with_restart_pending_goes_to_initial() {
        let mut sup = make_supervisor(vec![("svc", ServiceStatus::InKilling)]);
        // Simulate: service has a pid and restart_pending is set
        {
            let sh = sup.repo.get_mut_sh("svc");
            sh.restart_pending = true;
            sh.pid = Some(nix::unistd::Pid::from_raw(99999));
            sup.repo
                .add_pid(nix::unistd::Pid::from_raw(99999), "svc".into());
        }
        let events = sup.handle_event(Event::ServiceExited("svc".into(), 0));
        // Should produce StatusChanged to Initial
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], Event::StatusChanged(name, ServiceStatus::Initial) if name == "svc"),
            "Expected StatusChanged to Initial after restart_pending exit, got: {:?}",
            events[0]
        );
        let sh = sup.repo.get_sh("svc");
        assert_eq!(sh.status, ServiceStatus::Initial);
        assert!(!sh.restart_pending, "restart_pending should be cleared");
    }
}
