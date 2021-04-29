//! The supervisor is one of the biggest module. It is responsible for supervising the services, and
//! keeping track of their current state.
//! It will also reap the dead processes

use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, ExitStatus, Service, ServiceStatus, ShuttingDown};
use crate::horust::healthcheck;
use nix::sys::signal;
use nix::unistd;
use repo::Repo;
use service_handler::ServiceHandler;
use std::fmt::Debug;
use std::ops::Mul;
use std::thread;
use std::time::{Duration, Instant};

mod process_spawner;
mod reaper;
mod repo;
mod service_handler;
mod signal_handling;

pub(crate) use signal_handling::init;


/// How many pid reap per iteration of the reaper
const MAX_PROCESS_REAPS_ITERS: u32 = 20;

// Spawns and runs this component in a new thread.
pub fn spawn(
    bus: BusConnector<Event>,
    services: Vec<Service>,
) -> std::thread::JoinHandle<ExitStatus> {
    thread::spawn(move || Supervisor::new(bus, services).run())
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum LifecycleStatus {
    Running,
    ShuttingDown(ShuttingDown)
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
            status: LifecycleStatus::Running
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

                let has_failed = !service_handler
                    .service()
                    .failure
                    .successful_exit_code
                    .contains(&exit_code);

                // If it has failed too quickly, increase service_handler's restart attempts
                // and check if it has more attempts left.
                if service_handler.has_some_failed_healthchecks()
                    && service_handler.is_early_state()
                {
                    service_handler.restart_attempts += 1;
                }

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
                    self.status = LifecycleStatus::ShuttingDown(ShuttingDown::Gracefuly);
                    return vec![
                        Event::StatusUpdate(
                            service_handler.name().clone(),
                            ServiceStatus::FinishedFailed,
                        ),
                        Event::ShuttingDownInitiated(ShuttingDown::Gracefuly),
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
                kill(&service_handler, Some(signal::SIGKILL));
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
                // Count the failed healthiness checks. The state change producer wll handle states
                // changes (if they're needed)
                sh.add_healthcheck_event(health);
                vec![]
            }
            Event::ShuttingDownInitiated(shutting_down) => {
                match shutting_down {
                    ShuttingDown::Gracefuly => {
                        warn!("Gracefully stopping...");
                    }
                    ShuttingDown::Forcefuly => {
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

    /// Blocking call.
    /// This function will run the services and reap dead pids.
    fn run(mut self) -> ExitStatus {
        while !self.repo.all_have_finished() {
            // Ingest updates
            let received_events = self.repo.get_events();
            debug!("Applying events... {:?}", received_events);
            match (self.status, signal_handling::is_sigterm_received()) {
                (LifecycleStatus::Running, true) => {
                    warn!("1. SIGTERM received");
                    self.repo.send_ev(Event::ShuttingDownInitiated(ShuttingDown::Gracefuly));
                }
                (LifecycleStatus::ShuttingDown(ShuttingDown::Gracefuly), true) => {
                    warn!("2. SIGTERM received");
                    self.repo.send_ev(Event::ShuttingDownInitiated(ShuttingDown::Forcefuly));
                }
                _ => {}
            }
            // Handling of the received events and commands:
            let produced_events = received_events
                .into_iter()
                .map(|ev| self.handle_event(ev))
                .flatten()
                .collect::<Vec<Event>>();
            debug!("Produced events: {:?}", produced_events);
            // Producing commands which will be applied in the next iteration
            let next_evs: Vec<Event> = self
                .repo
                .services
                .iter()
                .map(|(_s_name, sh)| sh.next(&self.repo, self.status))
                .flatten()
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

            std::thread::sleep(Duration::from_millis(300));
        }

        debug!("All services have finished");
        // If we're the init system, let's be sure that everything stops before exiting.
        let init_pid = unistd::Pid::from_raw(1);
        // TODO: Test (probably via docker).
        if unistd::getpid() == init_pid {
            let all_processes = unistd::Pid::from_raw(-1);
            let _res = signal::kill(all_processes, signal::SIGTERM);
            thread::sleep(Duration::from_secs(3));
            let _res = signal::kill(all_processes, signal::SIGKILL);
        }

        self.repo.send_ev(Event::ShuttingDownInitiated(ShuttingDown::Gracefuly));
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
            match error.as_errno().expect("errno empty!") {
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
