use crate::horust::error::Result;
use crate::horust::formats::{FailureStrategy, Service, ServiceHandler, ServiceStatus};
use crate::horust::repository::ServiceRepository;
use crate::horust::{healthcheck, signal_handling};
use nix::sys::signal::kill;
use nix::sys::signal::Signal;
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
    service_repository: ServiceRepository,
    /// Instant representing at which time we received a shutdown request. Will be used for comparing Service.termination.wait
    shutting_down_start: Option<Instant>,
}

// Spawns and runs this component in a new thread.
pub fn spawn(repo: ServiceRepository) {
    thread::spawn(move || Runtime::new(repo).run());
}

impl Runtime {
    fn new(repo: ServiceRepository) -> Self {
        Self {
            service_repository: repo,
            shutting_down_start: None,
        }
    }

    fn check_is_shutting_down(&mut self) {
        // If any failed service has the kill-all strategy, then shut everything down.
        let should_shut_down = self
            .service_repository
            .get_failed()
            .any(|sh| sh.service().failure.strategy == FailureStrategy::KillAll);

        if signal_handling::is_sigterm_received() || should_shut_down {
            if self.shutting_down_start.is_none() {
                self.shutting_down_start = Some(Instant::now());
            }
            self.stop_all_services();
        }
    }

    // Blocking call
    pub fn run(&mut self) {
        loop {
            //TODO: a blocking update maybe? This loop should be executed onstatechange.
            self.service_repository.ingest("runtime");
            self.check_is_shutting_down();
            /*           self.service_repository
            .get_failed()
            .filter(|sh| sh.service().failure.strategy == FailureStrategy::KillDepdencies)
            .for_each(|sh| {
                self.service_repository
                    .get_dependencies(sh.name().into())
                    .iter()
                    .filter(|sh| !sh.is_in_killing())
                    .for_each(|sh| {
                        if sh.status != ServiceStatus::InKilling {

                        }
                    })
            });*/

            self.service_repository
                .get_runnable_services()
                .into_iter()
                .for_each(|service_handler| {
                    self.service_repository
                        .update_status(service_handler.name(), ServiceStatus::ToBeRun);
                    healthcheck::prepare_service(&service_handler).unwrap();
                    run_spawning_thread(
                        service_handler.service().clone(),
                        self.service_repository.clone(),
                    );
                });
            if self.service_repository.all_finished() {
                debug!("All services have finished, exiting...");
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        std::process::exit(0);
    }

    /**
    Send a term signal to all the services in the "Running" state.
    **/
    pub fn stop_all_services(&mut self) {
        let shutting_down_elapsed_secs = self
            .shutting_down_start
            .clone()
            .unwrap()
            .elapsed()
            .as_secs();
        let kill = |sh: &ServiceHandler, signal: Signal| {
            debug!("Going to send {} signal to pid {:?}", signal, sh.pid());
            let pid = sh
                .pid()
                .expect("Missing pid to kill but process was in running state.");
            match kill(pid, signal) {
                Err(error) => match error.as_errno().unwrap() {
                    nix::errno::Errno::ESRCH => (),
                    _ => error!(
                        "Error encountered while killing the process: {}, service: {}, pid: {:?}",
                        error,
                        sh.name(),
                        pid,
                    ),
                },
                Ok(()) => (),
            }
        };

        self.service_repository.mutate_service_status(|sh| {
            // If after termination.wait time the service has not yet exited, then we will use the force:
            let should_force_kill =
                shutting_down_elapsed_secs > sh.service().termination.wait.clone().as_secs();

            if sh.is_in_killing() && should_force_kill {
                kill(sh, Signal::SIGKILL);
                sh.set_status(ServiceStatus::Finished);
                Some(sh)
            } else if sh.is_running() && sh.pid().is_some() {
                kill(sh, sh.service().termination.signal.as_signal());
                sh.set_status(ServiceStatus::InKilling);
                Some(sh)
            } else if sh.is_initial() {
                sh.set_status(ServiceStatus::Finished);
                Some(sh)
            } else {
                None
            }
        });
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

    std::env::set_current_dir(cwd).unwrap();
    nix::unistd::setsid().unwrap();
    nix::unistd::setuid(service.user.get_uid()).unwrap();
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
