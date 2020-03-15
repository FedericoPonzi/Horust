use crate::horust::error::Result;
use crate::horust::formats::{FailureStrategy, Service, ServiceHandler, ServiceStatus};
use crate::horust::repository::ServiceRepository;
use crate::horust::{healthcheck, signal_handling};
use nix::sys::signal::{self, Signal};
use nix::unistd::{fork, getppid, ForkResult};
use nix::unistd::{getpid, Pid};
use shlex;
use std::ffi::{CStr, CString};
use std::fmt::Debug;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub struct Runtime {
    service_repository: ServiceRepository,
    is_shutting_down: bool,
}

// Spawns and runs this component in a new thread.
pub fn spawn(repo: ServiceRepository) {
    thread::spawn(move || Runtime::new(repo).run());
}

impl Runtime {
    fn new(repo: ServiceRepository) -> Self {
        Self {
            service_repository: repo,
            is_shutting_down: false,
        }
    }

    /// Check if the system should shutdown. If so, triggers the stop of all the services.
    fn should_shutdown(&mut self) {
        if self.is_shutting_down {
            self.stop_all_services();
        } else {
            // If any failed service has the kill-all strategy, then shut everything down.
            let failed_shutdown_strategy = self
                .service_repository
                .get_failed()
                .into_iter()
                .any(|sh| sh.service().failure.strategy == FailureStrategy::Shutdown);

            if failed_shutdown_strategy {
                debug!("Found failed service with shutdown strategy.");
            }
            if signal_handling::is_sigterm_received() || failed_shutdown_strategy {
                self.is_shutting_down = true;
                self.stop_all_services();
            }
        }
    }
    pub fn handle_failing_service(
        &mut self,
        mut failed_sh: ServiceHandler,
    ) -> Option<ServiceHandler> {
        match failed_sh.service().failure.strategy {
            FailureStrategy::Shutdown => {
                self.is_shutting_down = true;
                None
            }
            FailureStrategy::KillDependents => {
                debug!("Failed service has kill-dependents strategy, going to mark them all..");
                let _dependents = self
                    .service_repository
                    .get_dependents(failed_sh.name().into())
                    .iter_mut()
                    .for_each(|dep| {
                        dep.marked_for_killing = true;
                        self.service_repository.mutate_marked_for_killing(Some(dep))
                    });

                // Todo: finishedfailed
                failed_sh.set_status(ServiceStatus::Finished);
                Some(failed_sh)
            }
            _ => None,
        }
    }
    fn handle_runnable_service(
        &mut self,
        service_handler: ServiceHandler,
    ) -> Option<ServiceHandler> {
        debug!("Found runnable service: {:?}", service_handler);
        self.service_repository
            .update_status(service_handler.name(), ServiceStatus::ToBeRun);
        healthcheck::prepare_service(&service_handler).unwrap();
        run_spawning_thread(
            service_handler.service().clone(),
            self.service_repository.clone(),
        );
        None
    }
    fn handle_in_killing_service(
        &self,
        mut service_handler: ServiceHandler,
    ) -> Option<ServiceHandler> {
        debug!("{} is in killing..", service_handler.name());
        // If after termination.wait time the service has not yet exited, then we will use the force:
        let should_force_kill =
            if let Some(shutting_down_elapsed_secs) = service_handler.shutting_down_start.clone() {
                let shutting_down_elapsed_secs = shutting_down_elapsed_secs.elapsed().as_secs();

                debug!(
                    "{}, should not force kill. Elapsed: {}, termination wait: {}",
                    service_handler.name(),
                    shutting_down_elapsed_secs,
                    service_handler.service().termination.wait.clone().as_secs()
                );
                shutting_down_elapsed_secs
                    > service_handler.service().termination.wait.clone().as_secs()
            } else {
                error!("There is no shutting down elapsed secs!!");
                false
            };
        if service_handler.is_in_killing() && should_force_kill {
            kill(&service_handler, Signal::SIGKILL);
            service_handler.set_status(ServiceStatus::Finished);
            service_handler.shutting_down_start = None;
            Some(service_handler)
        } else {
            None
        }
    }
    /// Blocking call. Continuously try to (re)start services, and init shutdown as needed.
    pub fn run(&mut self) {
        loop {
            //TODO: a blocking update maybe? This loop should be executed onstatechange.
            // Ingest updates
            self.service_repository.ingest("runtime");

            // Check if the system is shuttingdown and if so handles the shutdown of the services.
            self.should_shutdown();
            let old_repo = self.service_repository.clone();
            old_repo.services.into_iter().for_each(|mut sh| {
                let sh = if sh.is_failed() {
                    self.handle_failing_service(sh)
                } else if self.service_repository.is_service_runnable(&sh) {
                    self.handle_runnable_service(sh)
                } else if sh.marked_for_killing {
                    shutdown_service(&mut sh);
                    Some(sh)
                } else if sh.is_in_killing() {
                    self.handle_in_killing_service(sh)
                } else {
                    None
                };
                self.service_repository.mutate_service_status(sh.as_ref());
            });
            // If some process has failed, applies the failure strategies.
            if self.service_repository.all_finished() {
                debug!("All services have finished, exiting...");
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        std::process::exit(0);
    }

    /// Send a term signal to all the services in the "Running" state.
    pub fn stop_all_services(&mut self) {
        self.service_repository
            .mutate_service_status_apply(shutdown_service);
    }
}

/// Handle the shutting down of a service. It returns Some if it has modified the sh.
fn shutdown_service(sh: &mut ServiceHandler) -> Option<&ServiceHandler> {
    debug!(
        "Shutting down service: {}, status: {}",
        sh.name(),
        sh.status
    );
    if sh.is_running() && sh.pid().is_some() {
        kill(sh, sh.service().termination.signal.as_signal());
        sh.shutting_down_started();
        sh.set_status(ServiceStatus::InKilling);
        Some(sh)
    } else if sh.is_initial() {
        sh.marked_for_killing = false;
        sh.set_status(ServiceStatus::Finished);
        Some(sh)
    } else {
        None
    }
}

fn kill(sh: &ServiceHandler, signal: Signal) {
    debug!("Going to send {} signal to pid {:?}", signal, sh.pid());
    let pid = sh
        .pid()
        .expect("Missing pid to kill but process was in running state.");
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
}

/// Run another thread that will wait for the start delay, and handle the fork / exec.
fn run_spawning_thread(service: Service, mut service_repository: ServiceRepository) {
    std::thread::spawn(move || {
        std::thread::sleep(service.start_delay);
        match spawn_process(&service) {
            Ok(pid) => {
                debug!("Setting pid:{} for service: {}", pid, service.name);
                service_repository.update_pid(service.name, pid);
            }
            Err(error) => {
                error!("Failed spawning the process: {}", error);
                service_repository.update_status(&service.name, ServiceStatus::Failed);
            }
        }
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
