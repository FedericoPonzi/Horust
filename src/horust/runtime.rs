use crate::horust::bus::BusConnector;
use crate::horust::error::Result;
use crate::horust::formats::{
    Event, FailureStrategy, Service, ServiceHandler, ServiceName, ServiceStatus,
};
use crate::horust::{healthcheck, signal_handling};
use nix::sys::signal::{self, Signal};
use nix::unistd::{fork, getppid, ForkResult};
use nix::unistd::{getpid, Pid};
use shlex;
use std::ffi::{CStr, CString};
use std::fmt::Debug;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Runtime {
    is_shutting_down: bool,
    repo: Repo,
}

#[derive(Debug, Clone)]
struct Repo {
    pub services: Vec<ServiceHandler>,
    pub(crate) bus: BusConnector,
}

impl Repo {
    fn new<T: Into<ServiceHandler>>(bus: BusConnector, services: Vec<T>) -> Self {
        let services = services.into_iter().map(Into::into).collect();
        Self { bus, services }
    }
    fn get_events(&mut self) -> Vec<Event> {
        self.bus.try_get_events()
    }
    pub fn all_finished(&self) -> bool {
        self.services
            .iter()
            .all(|sh| sh.is_finished() || sh.is_failed())
    }
    pub fn get_mut_service(&mut self, service_name: &ServiceName) -> &mut ServiceHandler {
        self.services
            .iter_mut()
            .filter(|sh| sh.name() == service_name)
            .last()
            .unwrap()
    }

    fn get_dependents(&self, service_name: &ServiceName) -> Vec<ServiceHandler> {
        self.services
            .iter()
            .filter(|sh| sh.service().start_after.contains(service_name))
            .cloned()
            .collect()
    }

    fn send_ev(&mut self, ev: Event) {
        self.bus.send_event(ev)
    }

    fn is_service_runnable(&self, sh: &ServiceHandler) -> bool {
        if !sh.is_initial() {
            return false;
        }
        //TODO: check if it's finished failed. Apply restart policy.

        for service_name in sh.start_after() {
            let is_started = self.services.iter().any(|service| {
                service.name() == service_name && (service.is_running() || service.is_finished())
            });
            if !is_started {
                return false;
            }
        }
        true
    }
}

// Spawns and runs this component in a new thread.
pub fn spawn(bus: BusConnector, services: Vec<Service>) {
    thread::spawn(move || Runtime::new(bus, services).run());
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
                                "Service tobekilled was in status: {}",
                                service_handler.status
                            );
                        }
                    }
                    ServiceStatus::ToBeRun => {
                        if service_handler.status == ServiceStatus::Initial {
                            service_handler.status = ServiceStatus::ToBeRun;
                            healthcheck::prepare_service(service_handler).unwrap();
                            run_spawning_thread(
                                service_handler.service().clone(),
                                self.repo.clone(),
                            );
                        } else {
                            debug!("{}: Ignoring ToBeRun event", service_name);
                        }
                    }
                    ServiceStatus::Running => {
                        if service_handler.status == ServiceStatus::Starting {
                            service_handler.status = ServiceStatus::Running;
                        }
                    }
                    ServiceStatus::Starting => {
                        if service_handler.status != ServiceStatus::InKilling {
                            service_handler.status = ServiceStatus::Starting;
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
                service_handler.set_status_by_exit_code(exit_code);
            }
            Event::ForceKill(service_name) => {
                let service_handler = self.repo.get_mut_service(&service_name);
                kill(&service_handler, Signal::SIGKILL);
                service_handler.status = ServiceStatus::Finished;
            }
            Event::PidChanged(service_name, pid) => {
                let service_handler = self.repo.get_mut_service(&service_name);
                service_handler.pid = Some(pid);
            }
            Event::ShuttingDownInitiated => self.is_shutting_down = true,
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
                ServiceStatus::Failed => handle_failed_service(&self.repo, service_handler),
                ServiceStatus::InKilling => {
                    if should_force_kill(service_handler) {
                        vec![Event::new_force_kill(service_handler.name())]
                    } else {
                        vec![]
                    }
                }
                ServiceStatus::Running | ServiceStatus::Starting => {
                    // Change to service in killing event.
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
    pub fn run(&mut self) {
        loop {
            // Ingest updates
            let events = self.repo.get_events();
            debug!("Applying events.. {:?}", events);
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
            events.into_iter().for_each(|ev| self.repo.send_ev(ev));
            // If some process has failed, applies the failure strategies.
            if self.repo.all_finished() {
                debug!("All services have finished, exiting...");
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        std::process::exit(0);
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

fn handle_failed_service(repo: &Repo, failed_sh: &ServiceHandler) -> Vec<Event> {
    match failed_sh.service().failure.strategy {
        FailureStrategy::Shutdown => vec![Event::ShuttingDownInitiated],
        FailureStrategy::KillDependents => {
            debug!("Failed service has kill-dependents strategy, going to mark them all..");
            let finished_ev = vec![Event::new_status_changed(
                failed_sh.name(),
                // Todo: finishedfailed
                ServiceStatus::Finished,
            )];
            repo.get_dependents(failed_sh.name().into())
                .iter()
                .map(|sh| Event::new_status_changed(sh.name(), ServiceStatus::ToBeKilled))
                .chain(finished_ev)
                .collect()
        }
        _ => vec![],
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
fn run_spawning_thread(service: Service, mut repo: Repo) {
    std::thread::spawn(move || {
        std::thread::sleep(service.start_delay);
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
    let default = PathBuf::from("/");
    let cwd = service.working_directory.as_ref().unwrap_or(&default);
    debug!("Set cwd: {:?}, ", cwd);

    std::env::set_current_dir(cwd).expect("Set cwd");
    nix::unistd::setsid().expect("Setsid");
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
    nix::unistd::execvpe(program_name.as_ref(), arg_cptr.as_ref(), env_cptr.as_ref())
        .expect("Execvp() failed: ");
}

#[cfg(test)]
mod test {
    use crate::horust::formats::{Service, ServiceHandler};
    use crate::horust::runtime::should_force_kill;
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
}
