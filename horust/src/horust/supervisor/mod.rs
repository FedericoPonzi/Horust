//! The supervisor is one of the biggest modules. It is responsible for supervising the services, and
//! keeping track of their current state.
//! It will also reap the dead processes

use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Mul;
use std::thread;
use std::time::Duration;

use crossbeam::channel::{Receiver, TryRecvError};
use nix::sys::signal;
use nix::unistd;

pub(crate) use process_spawner::find_program;
use repo::Repo;
use service_handler::ServiceHandler;
pub(crate) use signal_handling::init;

use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, ExitStatus, Restart, Service, ServiceStatus, ShuttingDown};
use crate::horust::healthcheck;

mod process_spawner;
mod reaper;
mod repo;
mod service_handler;
mod signal_handling;

#[cfg(all(test, target_os = "linux"))]
mod registration_tests;

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

/// Caps the restart delay when the attempts budget is unbounded. Matches the number of
/// attempts Horust used to default to, so the longest delay stays in a familiar range.
const MAX_BACKOFF_MULTIPLIER: u32 = 10;

/// Delay before respawning a service: it grows with the number of rapid failures, so that
/// a service failing over and over is retried increasingly slowly.
/// The multiplier is capped because `restart_attempts` is only reset once the service
/// becomes stable: with an unbounded budget (`attempts = 0`) it would otherwise grow
/// forever and push restarts infinitely far apart.
fn restart_backoff(restart: &Restart, restart_attempts: u32) -> Duration {
    let cap = if restart.attempts > 0 {
        restart.attempts
    } else {
        MAX_BACKOFF_MULTIPLIER
    };
    restart.backoff.mul(restart_attempts.min(cap))
}

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
    preparations: HashMap<String, Receiver<process_spawner::Preparation>>,
}

impl Supervisor {
    fn new(bus: BusConnector<Event>, services: Vec<Service>) -> Self {
        let repo = Repo::new(bus, services);
        Self {
            repo,
            status: LifecycleStatus::Running,
            preparations: HashMap::new(),
        }
    }

    /// Handle the events, returns Events (state changes) to be dispatched.
    fn handle_event(&mut self, ev: Event) -> Vec<Event> {
        match ev {
            // Reaped exits are applied locally; their bus echoes are notifications only.
            Event::ServiceExited(service_name, exit_code)
                if self.repo.get_sh(&service_name).pid.is_some() =>
            {
                let pid = self.repo.get_sh(&service_name).pid.unwrap();
                self.repo.remove_pid(pid);
                let service_handler = self.repo.get_mut_sh(&service_name);
                service_handler.shutting_down_start = None;
                service_handler.pid = None;

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
                    || service_handler.exceeded_max_failed_healthchecks()
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
                let Some(attempt_id) = service_handler.attempt_id.checked_add(1) else {
                    error!("Attempt IDs exhausted for service: {service_name}");
                    service_handler.status = ServiceStatus::FinishedFailed;
                    let mode =
                        if self.status == LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully) {
                            ShuttingDown::Forcefully
                        } else {
                            ShuttingDown::Gracefully
                        };
                    let shutdown = Event::ShuttingDownInitiated(mode);
                    self.handle_event(shutdown.clone());
                    return vec![
                        Event::new_status_changed(&service_name, ServiceStatus::FinishedFailed),
                        shutdown,
                    ];
                };
                if self.status != LifecycleStatus::Running {
                    debug!("Discarding late Run during shutdown for service: {service_name}");
                    return vec![];
                }
                service_handler.attempt_id = attempt_id;
                service_handler.status = ServiceStatus::Starting;
                // Health check results from a previous run must not carry over to the new
                // process, otherwise a service that was healthy before would immediately
                // be considered green again (skipping its `healthy-after` window).
                service_handler.healthiness_checks_failed = None;
                let evs = vec![Event::StatusChanged(
                    service_name.clone(),
                    ServiceStatus::Starting,
                )];

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
                let backoff = restart_backoff(
                    &service_handler.service().restart,
                    service_handler.restart_attempts,
                );
                let receiver = process_spawner::prepare_service(
                    service_handler.service().clone(),
                    backoff,
                    self.repo.bus.join_bus(),
                );
                self.preparations.insert(service_name, receiver);
                evs
            }
            Event::SpawnFailed(s_name) => {
                let service_handler = self.repo.get_mut_sh(&s_name);
                // The process never came to exist, so no ServiceExited will ever arrive to
                // account for this failure. Count it here, otherwise a service that can
                // never be spawned (e.g. command not found) would restart forever.
                service_handler.restart_attempts += 1;
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
                vec![]
            }
            Event::HealthCheck(s_name, attempt_id, health) => {
                let sh = self.repo.get_mut_sh(&s_name);
                if attempt_id != sh.attempt_id {
                    debug!(
                        "Discarding health result from obsolete attempt {attempt_id} for {s_name}"
                    );
                    return vec![];
                }
                // Count the failed healthiness checks. The state change producer will handle states
                // changes (if they're needed)
                sh.add_healthcheck_event(health);
                vec![]
            }
            Event::ShuttingDownInitiated(shutting_down) => {
                if self.status == LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully) {
                    return vec![];
                }
                match shutting_down {
                    ShuttingDown::Gracefully => {
                        warn!("Gracefully stopping...");
                    }
                    ShuttingDown::Forcefully => {
                        warn!("Terminating all services...");
                    }
                }
                self.status = LifecycleStatus::ShuttingDown(shutting_down);
                vec![]
            }
            Event::StatusUpdate(service_name, new_status) => {
                let service_handler = self.repo.get_sh(&service_name);
                if service_handler.status == ServiceStatus::Started
                    && new_status == ServiceStatus::Running
                    && service_handler.has_some_failed_healthchecks()
                {
                    debug!("Discarding stale Running update for unready service: {service_name}");
                    return vec![];
                }
                if new_status == ServiceStatus::Initial
                    && self.status != LifecycleStatus::Running
                    && matches!(
                        service_handler.status,
                        ServiceStatus::Success | ServiceStatus::Failed
                    )
                {
                    debug!("Discarding queued restart during shutdown for service: {service_name}");
                    return vec![];
                }

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

    fn spawn_prepared(&mut self) -> Vec<Event> {
        let completed = self
            .preparations
            .iter()
            .filter_map(|(name, receiver)| match receiver.try_recv() {
                Ok(result) => Some((name.clone(), result)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some((
                    name.clone(),
                    Err(anyhow::anyhow!("preparation worker disconnected")),
                )),
            })
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for (name, result) in completed {
            self.preparations.remove(&name);
            let handler = &self.repo.services[&name];
            if handler.pid.is_some()
                || !matches!(
                    handler.status,
                    ServiceStatus::Starting | ServiceStatus::InKilling
                )
            {
                debug!("Discarding preparation for obsolete attempt: {name}");
                continue;
            }
            match result {
                Ok(Some(prepared))
                    if self.status == LifecycleStatus::Running
                        && handler.status == ServiceStatus::Starting =>
                {
                    match process_spawner::spawn_process(prepared) {
                        Ok(pid) => events.extend(self.register_spawned(name, pid)),
                        Err(error) => {
                            error!("Failed spawning the process: {error}");
                            events.extend(self.handle_event(Event::SpawnFailed(name)));
                        }
                    }
                }
                Ok(Some(_)) if handler.status == ServiceStatus::InKilling => {
                    events.extend(
                        self.handle_event(Event::StatusUpdate(name, ServiceStatus::Failed)),
                    );
                }
                result => {
                    if let Err(error) = result {
                        error!("Failed spawning the process: {error}");
                    }
                    events.extend(self.handle_event(Event::SpawnFailed(name)));
                }
            }
        }
        events
    }

    fn register_spawned(&mut self, name: String, pid: unistd::Pid) -> Vec<Event> {
        // Registration is local; the bus echoes PidChanged as a notification only.
        self.repo.add_pid(pid, name.clone());
        let handler = self.repo.get_mut_sh(&name);
        handler.pid = Some(pid);
        handler.status = ServiceStatus::Started;
        vec![
            Event::new_pid_changed(name.clone(), pid),
            Event::StatusChanged(name.clone(), ServiceStatus::Started),
            Event::StartHealthCheck(name, handler.attempt_id),
        ]
    }

    fn handle_signals(&mut self, pending: u32) {
        let shutdown = match (self.status, pending) {
            (_, 0) | (LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully), _) => return,
            (LifecycleStatus::Running, 1) => ShuttingDown::Gracefully,
            _ => ShuttingDown::Forcefully,
        };
        let event = Event::ShuttingDownInitiated(shutdown);
        // Apply locally before broadcasting so another signal need not wait for the bus.
        self.handle_event(event.clone());
        self.repo.send_ev(event);
    }

    /// Blocking call.
    /// This function will run the services and reap dead pids.
    fn run(mut self) -> ExitStatus {
        while !self.repo.all_have_finished() {
            // Ingest updates
            let received_events = self.repo.get_events();
            debug!("Applying events... {:?}", received_events);
            // Handling of the received events and commands:
            let mut produced_events = received_events
                .into_iter()
                .flat_map(|ev| self.handle_event(ev))
                .collect::<Vec<Event>>();
            debug!("Produced events: {:?}", produced_events);
            self.handle_signals(signal_handling::take_pending_signals());
            produced_events.extend(self.spawn_prepared());
            // Account for reaped PIDs before any delayed bus commands can target them.
            let reaped_events = reaper::run(&self.repo, MAX_PROCESS_REAPS_ITERS);
            for event in &reaped_events {
                produced_events.extend(self.handle_event(event.clone()));
            }
            produced_events.extend(reaped_events);
            // Producing commands which will be applied in the next iteration
            let next_evs: Vec<Event> = self
                .repo
                .services
                .iter()
                .flat_map(|(_s_name, sh)| sh.next(&self.repo, self.status))
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
mod tests {
    use std::time::Duration;

    use super::restart_backoff;
    use crate::horust::formats::{Restart, RestartStrategy};

    fn restart(attempts: u32, backoff_ms: u64) -> Restart {
        Restart {
            strategy: RestartStrategy::OnFailure,
            backoff: Duration::from_millis(backoff_ms),
            attempts,
        }
    }

    #[test]
    fn test_restart_backoff_grows_with_attempts() {
        let r = restart(5, 100);
        assert_eq!(restart_backoff(&r, 0), Duration::from_millis(0));
        assert_eq!(restart_backoff(&r, 3), Duration::from_millis(300));
    }

    #[test]
    fn test_restart_backoff_is_capped() {
        // restart_attempts is only reset once a service becomes stable, so with an
        // unbounded budget it grows forever: the delay must not grow with it.
        let unbounded = restart(0, 100);
        assert_eq!(
            restart_backoff(&unbounded, 10_000),
            restart_backoff(&unbounded, 10),
            "an unbounded budget must not push restarts infinitely far apart"
        );
        // With a budget the multiplier can never exceed it anyway.
        let bounded = restart(5, 100);
        assert_eq!(restart_backoff(&bounded, 99), Duration::from_millis(500));
    }
}

#[cfg(test)]
mod signal_tests {
    use super::*;
    use crate::horust::bus::Bus;

    #[test]
    fn signals_escalate_without_waiting_for_bus_events() {
        let bus = Bus::<Event>::new();
        let mut supervisor = Supervisor::new(bus.join_bus(), vec![]);
        supervisor.handle_signals(0);
        assert_eq!(supervisor.status, LifecycleStatus::Running);
        supervisor.handle_signals(1);
        assert_eq!(
            supervisor.status,
            LifecycleStatus::ShuttingDown(ShuttingDown::Gracefully)
        );
        supervisor.handle_signals(0);
        assert_eq!(
            supervisor.status,
            LifecycleStatus::ShuttingDown(ShuttingDown::Gracefully)
        );
        supervisor.handle_signals(1);
        assert_eq!(
            supervisor.status,
            LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully)
        );
        supervisor.handle_event(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
        assert_eq!(
            supervisor.status,
            LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully)
        );
        supervisor.handle_signals(2);
        assert_eq!(
            supervisor.status,
            LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully)
        );
    }

    #[test]
    fn two_pending_signals_force_shutdown() {
        let bus = Bus::<Event>::new();
        let mut supervisor = Supervisor::new(bus.join_bus(), vec![]);
        supervisor.handle_signals(2);
        assert_eq!(
            supervisor.status,
            LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully)
        );
    }

    #[test]
    fn signal_escalates_shutdown_initiated_by_service_failure() {
        let bus = Bus::<Event>::new();
        let mut supervisor = Supervisor::new(bus.join_bus(), vec![]);
        supervisor.handle_event(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
        supervisor.handle_signals(1);
        assert_eq!(
            supervisor.status,
            LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully)
        );
    }
}

#[cfg(test)]
mod health_attempt_tests {
    use super::*;
    use crate::horust::bus::Bus;
    use crate::horust::formats::{HealthinessStatus, RestartStrategy};

    fn supervisor() -> Supervisor {
        let mut service = Service::from_command("/bin/true".into());
        service.name = "a".into();
        service.restart.strategy = RestartStrategy::OnFailure;
        service.restart.attempts = 3;
        service.healthiness.max_failed = 1;
        let mut dependent = Service::from_name("b");
        dependent.start_after = vec!["a".into()];
        let bus = Bus::<Event>::new();
        Supervisor::new(bus.join_bus(), vec![service, dependent])
    }

    fn begin_attempt(supervisor: &mut Supervisor) {
        supervisor.handle_event(Event::Run("a".into()));
        assert!(
            supervisor
                .preparations
                .remove("a")
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn queued_old_health_does_not_affect_restarted_attempt() {
        let mut mutations = vec![];
        for registered in [false, true] {
            for health in [HealthinessStatus::Healthy, HealthinessStatus::Unhealthy] {
                let mut supervisor = supervisor();
                begin_attempt(&mut supervisor);
                let pid = unistd::Pid::from_raw(4242);
                supervisor.register_spawned("a".into(), pid);
                let old_attempt = supervisor.repo.services["a"].attempt_id;
                supervisor.handle_event(Event::HealthCheck(
                    "a".into(),
                    old_attempt,
                    HealthinessStatus::Healthy,
                ));
                supervisor.repo.get_mut_sh("a").restart_attempts = 2;
                let queued = Event::HealthCheck("a".into(), old_attempt, health.clone());
                supervisor.handle_event(Event::ServiceExited("a".into(), -137));
                let restart =
                    supervisor.repo.services["a"].next(&supervisor.repo, supervisor.status);
                assert_eq!(
                    restart,
                    vec![Event::new_status_update("a", ServiceStatus::Initial)]
                );
                supervisor.handle_event(restart.into_iter().next().unwrap());
                begin_attempt(&mut supervisor);
                assert_ne!(supervisor.repo.services["a"].attempt_id, old_attempt);
                if registered {
                    // Reusing the PID must not make an old worker's result current.
                    supervisor.register_spawned("a".into(), pid);
                }
                let before = supervisor.repo.services["a"].clone();
                assert_eq!(before.healthiness_checks_failed, None);
                let outputs = supervisor.handle_event(queued.clone());
                if supervisor.repo.services["a"] != before {
                    mutations.push((registered, health, supervisor.repo.services["a"].clone()));
                }
                assert!(outputs.is_empty());
                assert!(
                    supervisor.repo.services["b"]
                        .next(&supervisor.repo, supervisor.status)
                        .is_empty()
                );
                assert!(
                    supervisor.repo.services["a"]
                        .next(&supervisor.repo, supervisor.status)
                        .is_empty()
                );
                if !registered {
                    supervisor.register_spawned("a".into(), pid);
                }
                let current_attempt = supervisor.repo.services["a"].attempt_id;
                supervisor.handle_event(Event::HealthCheck(
                    "a".into(),
                    current_attempt,
                    HealthinessStatus::Healthy,
                ));
                let ready = supervisor.repo.services["a"].next(&supervisor.repo, supervisor.status);
                assert_eq!(
                    ready,
                    vec![Event::new_status_update("a", ServiceStatus::Running)]
                );
                supervisor.handle_event(ready.into_iter().next().unwrap());
                assert_eq!(supervisor.repo.services["a"].restart_attempts, 0);
                assert_eq!(
                    supervisor.repo.services["b"].next(&supervisor.repo, supervisor.status),
                    vec![Event::Run("b".into())]
                );
                let running = supervisor.repo.services["a"].clone();
                assert!(supervisor.handle_event(queued).is_empty());
                assert_eq!(supervisor.repo.services["a"], running);
            }
        }
        assert!(
            mutations.is_empty(),
            "old results mutated new attempts: {mutations:?}"
        );
    }

    #[test]
    fn current_health_preserves_readiness_and_failure_thresholds() {
        let mut supervisor = supervisor();
        begin_attempt(&mut supervisor);
        supervisor.register_spawned("a".into(), unistd::Pid::from_raw(4242));
        let attempt = supervisor.repo.services["a"].attempt_id;
        supervisor.repo.get_mut_sh("a").restart_attempts = 2;
        supervisor.handle_event(Event::HealthCheck(
            "a".into(),
            attempt,
            HealthinessStatus::Healthy,
        ));
        let ready = supervisor.repo.services["a"].next(&supervisor.repo, supervisor.status);
        assert_eq!(
            ready,
            vec![Event::new_status_update("a", ServiceStatus::Running)]
        );
        assert_eq!(
            supervisor.handle_event(ready.into_iter().next().unwrap()),
            vec![Event::new_status_changed("a", ServiceStatus::Running)]
        );
        assert_eq!(supervisor.repo.services["a"].restart_attempts, 0);
        assert_eq!(supervisor.repo.services["a"].attempt_id, attempt);
        assert_eq!(
            supervisor.repo.services["b"].next(&supervisor.repo, supervisor.status),
            vec![Event::Run("b".into())]
        );
        for _ in 0..2 {
            supervisor.handle_event(Event::HealthCheck(
                "a".into(),
                attempt,
                HealthinessStatus::Unhealthy,
            ));
        }
        assert_eq!(
            supervisor.repo.services["a"].healthiness_checks_failed,
            Some(2)
        );
        assert_eq!(
            supervisor.repo.services["a"].next(&supervisor.repo, supervisor.status),
            vec![
                Event::new_status_update("a", ServiceStatus::InKilling),
                Event::Kill("a".into())
            ]
        );
        assert_eq!(supervisor.repo.services["a"].restart_attempts, 0);
    }

    #[test]
    fn exhausted_attempt_ids_fail_without_wrapping_or_downgrading_shutdown() {
        for phase in [
            LifecycleStatus::Running,
            LifecycleStatus::ShuttingDown(ShuttingDown::Gracefully),
            LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully),
        ] {
            let mut supervisor = supervisor();
            supervisor.status = phase;
            let sh = supervisor.repo.get_mut_sh("a");
            sh.attempt_id = u64::MAX;
            sh.healthiness_checks_failed = Some(2);
            sh.restart_attempts = 2;
            let mut expected = sh.clone();
            expected.status = ServiceStatus::FinishedFailed;
            let shutdown = if phase == LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully) {
                ShuttingDown::Forcefully
            } else {
                ShuttingDown::Gracefully
            };
            assert_eq!(
                supervisor.handle_event(Event::Run("a".into())),
                vec![
                    Event::new_status_changed("a", ServiceStatus::FinishedFailed),
                    Event::ShuttingDownInitiated(shutdown),
                ]
            );
            assert_eq!(supervisor.repo.services["a"], expected);
            assert_eq!(supervisor.status, LifecycleStatus::ShuttingDown(shutdown));
            assert!(supervisor.preparations.is_empty());
            assert!(supervisor.repo.pid_map.is_empty());
        }
    }
}

#[cfg(test)]
mod run_admission_tests {
    use super::*;
    use crate::horust::bus::Bus;

    fn initial_service(attempt_id: u64) -> (Supervisor, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("healthy");
        std::fs::write(&path, "healthy").unwrap();
        let mut service = Service::from_name("svc");
        service.command = "/bin/true".into();
        service.healthiness.file_path = Some(path);
        let bus = Bus::<Event>::new();
        let mut supervisor = Supervisor::new(bus.join_bus(), vec![service]);
        let sh = supervisor.repo.get_mut_sh("svc");
        sh.attempt_id = attempt_id;
        sh.restart_attempts = 2;
        sh.healthiness_checks_failed = Some(2);
        (supervisor, dir)
    }

    #[test]
    fn queued_run_preserves_unstarted_service_during_shutdown() {
        for shutdown in [ShuttingDown::Gracefully, ShuttingDown::Forcefully] {
            for attempt in [7, u64::MAX - 1] {
                let (mut supervisor, dir) = initial_service(attempt);
                let queued =
                    supervisor.repo.services["svc"].next(&supervisor.repo, supervisor.status);
                assert_eq!(queued, vec![Event::Run("svc".into())]);
                supervisor.handle_event(Event::ShuttingDownInitiated(shutdown));
                let before = supervisor.repo.services["svc"].clone();
                let outputs = supervisor.handle_event(queued.into_iter().next().unwrap());
                assert_eq!(supervisor.repo.services["svc"], before, "{shutdown:?}");
                assert!(outputs.is_empty(), "late admission: {outputs:?}");
                assert_eq!(supervisor.status, LifecycleStatus::ShuttingDown(shutdown));
                assert!(dir.path().join("healthy").exists());
                assert!(supervisor.preparations.is_empty());
                assert!(supervisor.repo.pid_map.is_empty());

                let finalization = before.next(&supervisor.repo, supervisor.status);
                assert_eq!(
                    finalization,
                    vec![Event::new_status_update("svc", ServiceStatus::Finished)]
                );
                assert_eq!(
                    supervisor.handle_event(finalization.into_iter().next().unwrap()),
                    vec![Event::new_status_changed("svc", ServiceStatus::Finished)]
                );
                let mut expected = before;
                expected.status = ServiceStatus::Finished;
                assert_eq!(supervisor.repo.services["svc"], expected);
                assert!(supervisor.repo.all_have_finished());
                assert!(!supervisor.repo.any_finished_failed());
            }
        }
    }

    #[test]
    fn running_admission_still_prepares_and_resets_health() {
        let (mut supervisor, dir) = initial_service(7);
        let queued = supervisor.repo.services["svc"].next(&supervisor.repo, supervisor.status);
        assert_eq!(queued, vec![Event::Run("svc".into())]);
        assert_eq!(
            supervisor.handle_event(queued.into_iter().next().unwrap()),
            vec![Event::new_status_changed("svc", ServiceStatus::Starting)]
        );
        let sh = &supervisor.repo.services["svc"];
        assert_eq!(sh.status, ServiceStatus::Starting);
        assert_eq!(sh.attempt_id, 8);
        assert_eq!(sh.healthiness_checks_failed, None);
        assert_eq!(sh.restart_attempts, 2);
        assert_eq!(supervisor.status, LifecycleStatus::Running);
        assert!(!dir.path().join("healthy").exists());
        assert!(supervisor.repo.pid_map.is_empty());
        assert!(
            supervisor.preparations["svc"]
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap()
                .is_some()
        );
    }
}

#[cfg(test)]
mod status_update_tests {
    use super::*;
    use crate::horust::bus::Bus;
    use crate::horust::formats::{FailureStrategy, HealthinessStatus, RestartStrategy};

    fn queued_restart_updates(
        status: ServiceStatus,
        failure_strategy: FailureStrategy,
    ) -> (Supervisor, Vec<Event>) {
        let failed = status == ServiceStatus::Failed;
        let shutdown = failed && failure_strategy == FailureStrategy::Shutdown;
        let mut service = Service::from_name("a");
        service.restart.strategy = if failed {
            RestartStrategy::OnFailure
        } else {
            RestartStrategy::Always
        };
        service.restart.attempts = 3;
        service.failure.strategy = failure_strategy;
        let bus = Bus::<Event>::new();
        let mut supervisor = Supervisor::new(bus.join_bus(), vec![service]);
        let sh = supervisor.repo.get_mut_sh("a");
        sh.status = status;
        sh.restart_attempts = 2;
        sh.attempt_id = 4;
        sh.healthiness_checks_failed = Some(if failed { 1 } else { 0 });
        let queued = supervisor.repo.services["a"].next(&supervisor.repo, supervisor.status);
        let mut expected = vec![];
        if shutdown {
            expected.push(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
        }
        expected.push(Event::new_status_update("a", ServiceStatus::Initial));
        assert_eq!(queued, expected);
        (supervisor, queued)
    }

    fn assert_queued_restart_rejected(status: ServiceStatus) {
        for shutdown in [ShuttingDown::Gracefully, ShuttingDown::Forcefully] {
            let (mut supervisor, mut queued) =
                queued_restart_updates(status.clone(), FailureStrategy::Shutdown);
            let restart = queued.pop().unwrap();
            for event in queued {
                assert!(supervisor.handle_event(event).is_empty());
            }
            let phase = LifecycleStatus::ShuttingDown(shutdown);
            if supervisor.status != phase {
                supervisor.handle_event(Event::ShuttingDownInitiated(shutdown));
            }
            let before = supervisor.repo.services["a"].clone();
            let outputs = supervisor.handle_event(restart);
            assert_eq!(supervisor.repo.services["a"], before, "{shutdown:?}");
            assert!(outputs.is_empty(), "restart notification: {outputs:?}");
            assert_eq!(supervisor.status, phase);
            assert!(supervisor.preparations.is_empty());
            assert!(supervisor.repo.pid_map.is_empty());

            let terminal = if status == ServiceStatus::Failed {
                ServiceStatus::FinishedFailed
            } else {
                ServiceStatus::Finished
            };
            let finalization = supervisor.repo.services["a"].next(&supervisor.repo, phase);
            assert_eq!(
                finalization,
                vec![Event::new_status_update("a", terminal.clone())]
            );
            for event in finalization {
                assert_eq!(
                    supervisor.handle_event(event),
                    vec![Event::new_status_changed("a", terminal.clone())]
                );
            }
            let mut expected = before;
            expected.status = terminal;
            assert_eq!(supervisor.repo.services["a"], expected);
            assert!(supervisor.repo.all_have_finished());
            assert_eq!(
                supervisor.repo.any_finished_failed(),
                status == ServiceStatus::Failed
            );
        }
    }

    #[test]
    fn queued_failed_restart_is_rejected_during_shutdown() {
        assert_queued_restart_rejected(ServiceStatus::Failed);
    }

    #[test]
    fn queued_successful_restart_is_rejected_during_shutdown() {
        assert_queued_restart_rejected(ServiceStatus::Success);
    }

    #[test]
    fn queued_restart_is_accepted_while_running() {
        for status in [ServiceStatus::Success, ServiceStatus::Failed] {
            let (mut supervisor, queued) = queued_restart_updates(status, FailureStrategy::Ignore);
            let mut expected = supervisor.repo.services["a"].clone();
            expected.status = ServiceStatus::Initial;
            assert_eq!(
                supervisor.handle_event(queued.into_iter().next().unwrap()),
                vec![Event::new_status_changed("a", ServiceStatus::Initial)]
            );
            assert_eq!(supervisor.repo.services["a"], expected);
            assert_eq!(supervisor.status, LifecycleStatus::Running);
            assert_eq!(
                expected.next(&supervisor.repo, supervisor.status),
                vec![Event::Run("a".into())]
            );
        }
    }

    fn queued_running_update() -> (Supervisor, Event) {
        let mut dependent = Service::from_name("b");
        dependent.start_after = vec!["a".into()];
        let bus = Bus::<Event>::new();
        let mut supervisor =
            Supervisor::new(bus.join_bus(), vec![Service::from_name("a"), dependent]);
        let sh = supervisor.repo.get_mut_sh("a");
        sh.status = ServiceStatus::Started;
        sh.restart_attempts = 3;
        let attempt = supervisor.repo.services["a"].attempt_id;
        supervisor.handle_event(Event::HealthCheck(
            "a".into(),
            attempt,
            HealthinessStatus::Healthy,
        ));
        let queued = supervisor.repo.services["a"].next(&supervisor.repo, supervisor.status);
        assert_eq!(
            queued,
            vec![Event::new_status_update("a", ServiceStatus::Running)]
        );
        (supervisor, queued.into_iter().next().unwrap())
    }

    #[test]
    fn queued_running_update_rechecks_intervening_health_failure() {
        let (mut supervisor, queued) = queued_running_update();
        let attempt = supervisor.repo.services["a"].attempt_id;
        supervisor.handle_event(Event::HealthCheck(
            "a".into(),
            attempt,
            HealthinessStatus::Unhealthy,
        ));
        let before = supervisor.repo.services["a"].clone();
        assert_eq!(before.healthiness_checks_failed, Some(1));
        assert!(!before.exceeded_max_failed_healthchecks());

        let outputs = supervisor.handle_event(queued);
        assert_eq!(supervisor.repo.services["a"], before);
        assert!(
            outputs.is_empty(),
            "stale readiness notification: {outputs:?}"
        );
        assert!(
            supervisor.repo.services["b"]
                .next(&supervisor.repo, supervisor.status)
                .is_empty()
        );
    }

    #[test]
    fn queued_running_update_admits_currently_healthy_service() {
        let (mut supervisor, queued) = queued_running_update();
        assert_eq!(
            supervisor.handle_event(queued),
            vec![Event::new_status_changed("a", ServiceStatus::Running)]
        );
        let sh = &supervisor.repo.services["a"];
        assert_eq!(sh.status, ServiceStatus::Running);
        assert_eq!(sh.healthiness_checks_failed, Some(0));
        assert_eq!(sh.restart_attempts, 0);
        assert_eq!(
            supervisor.repo.services["b"].next(&supervisor.repo, supervisor.status),
            vec![Event::Run("b".into())]
        );
    }
}

#[cfg(test)]
mod service_exited_tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::Command;

    use nix::unistd::Pid;

    use super::*;
    use crate::horust::bus::Bus;
    use crate::horust::formats::RestartStrategy;

    /// Health-kills a Started service that exits 0, returning the restart decision. It must
    /// count as a failed rapid restart, otherwise its failure strategy never runs (#335).
    fn never_healthy_cycle(supervisor: &mut Supervisor) -> Vec<Event> {
        let pid = Pid::from_raw(4242);
        supervisor.repo.add_pid(pid, "svc".into());
        let sh = supervisor.repo.get_mut_sh("svc");
        sh.status = ServiceStatus::Started;
        sh.pid = Some(pid);
        sh.healthiness_checks_failed = Some(4);
        supervisor.handle_event(Event::new_status_update("svc", ServiceStatus::InKilling));
        assert_eq!(
            supervisor.handle_event(Event::ServiceExited("svc".into(), 0)),
            vec![Event::StatusChanged("svc".into(), ServiceStatus::Failed)]
        );
        let sh = supervisor.repo.get_sh("svc").clone();
        sh.next(&supervisor.repo, LifecycleStatus::Running)
    }

    #[test]
    fn never_healthy_service_fails_and_exhausts_restart_attempts() {
        let mut svc = Service::from_name("svc");
        svc.restart.strategy = RestartStrategy::OnFailure;
        svc.restart.attempts = 1;
        let bus = Bus::<Event>::new();
        let mut supervisor = Supervisor::new(bus.join_bus(), vec![svc]);
        assert!(
            never_healthy_cycle(&mut supervisor)
                .contains(&Event::new_status_update("svc", ServiceStatus::Initial))
        );
        assert!(
            never_healthy_cycle(&mut supervisor).contains(&Event::new_status_update(
                "svc",
                ServiceStatus::FinishedFailed
            ))
        );
    }

    #[test]
    fn force_kill_waits_for_exit_accounting() {
        for phase in [
            LifecycleStatus::Running,
            LifecycleStatus::ShuttingDown(ShuttingDown::Gracefully),
            LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully),
        ] {
            for successful in [false, true] {
                let mut service = Service::from_name("svc");
                service.restart.strategy = RestartStrategy::OnFailure;
                service.restart.attempts = 3;
                service.failure.strategy = crate::horust::formats::FailureStrategy::Shutdown;
                if successful {
                    service.failure.successful_exit_code = vec![-137];
                }
                let bus = Bus::<Event>::new();
                let mut supervisor = Supervisor::new(bus.join_bus(), vec![service]);
                supervisor.status = phase;
                let mut child = Command::new("/bin/sleep").arg("60").spawn().unwrap();
                let pid = Pid::from_raw(child.id() as i32);
                supervisor.register_spawned("svc".into(), pid);
                let sh = supervisor.repo.get_mut_sh("svc");
                sh.status = ServiceStatus::InKilling;
                sh.healthiness_checks_failed = Some(0);
                sh.restart_attempts = 2;
                sh.shutting_down_started();
                let before = sh.clone();

                let first = supervisor.handle_event(Event::new_force_kill("svc"));
                let repeated = supervisor.handle_event(Event::new_force_kill("svc"));
                // Reap our own child, but deliberately delay the supervisor's exit accounting.
                assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
                assert_eq!(supervisor.repo.services["svc"], before);
                assert!(first.is_empty(), "{phase:?}: premature events: {first:?}");
                assert!(repeated.is_empty());
                assert_eq!(supervisor.repo.get_service_by_pid(pid), Some(&"svc".into()));
                assert!(!supervisor.repo.all_have_finished());
                let pending = before.next(&supervisor.repo, phase);
                assert_eq!(
                    pending,
                    if phase == LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully) {
                        vec![Event::new_force_kill("svc")]
                    } else {
                        vec![]
                    }
                );

                let status = if successful {
                    ServiceStatus::Success
                } else {
                    ServiceStatus::Failed
                };
                assert_eq!(
                    supervisor.handle_event(Event::ServiceExited("svc".into(), -137)),
                    vec![Event::new_status_changed("svc", status.clone())]
                );
                let sh = &supervisor.repo.services["svc"];
                assert_eq!(sh.pid, None);
                assert_eq!(sh.shutting_down_start, None);
                assert_eq!(sh.restart_attempts, 2);
                assert!(supervisor.repo.pid_map.is_empty());
                let mut expected = vec![];
                if phase == LifecycleStatus::Running && !successful {
                    expected.push(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
                }
                expected.push(Event::new_status_update(
                    "svc",
                    if phase == LifecycleStatus::Running {
                        if successful {
                            ServiceStatus::Finished
                        } else {
                            ServiceStatus::Initial
                        }
                    } else if successful {
                        ServiceStatus::Finished
                    } else {
                        ServiceStatus::FinishedFailed
                    },
                ));
                assert_eq!(sh.next(&supervisor.repo, phase), expected);
                assert!(
                    supervisor
                        .handle_event(Event::new_force_kill("svc"))
                        .is_empty()
                );
                assert!(
                    supervisor
                        .handle_event(Event::ServiceExited("svc".into(), -137))
                        .is_empty()
                );
                assert_eq!(supervisor.repo.services["svc"].status, status);
            }
        }
    }
}
