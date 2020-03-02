use crate::horust::error::Result;
use crate::horust::formats::{Service, ServiceStatus};
use crate::horust::service_handler::ServiceRepository;
use crate::horust::{healthcheck, signal_handling};
use nix::sys::signal::kill;
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
}

pub fn spawn(repo: ServiceRepository) {
    thread::spawn(move || Runtime::new(repo).run());
}

impl Runtime {
    fn new(repo: ServiceRepository) -> Self {
        Self {
            service_repository: repo,
        }
    }

    fn check_is_shutting_down(&mut self) {
        if signal_handling::is_sigterm_received()
            && self.service_repository.is_any_service_running()
        {
            println!("Going to stop all services..");
            self.stop_all_services();
        }
    }

    /// Main entrypoint
    pub fn run(&mut self) {
        loop {
            //TODO: a blocking update maybe? This loop should be executed onstatechange.
            self.service_repository.ingest("runtime");
            self.check_is_shutting_down();
            let runnable_services = self.service_repository.get_runnable_services();
            runnable_services.into_iter().for_each(|service_handler| {
                self.service_repository
                    .update_status(service_handler.name(), ServiceStatus::ToBeRun);
                healthcheck::prepare_service(&service_handler).unwrap();
                run_spawning_thread(
                    service_handler.service().clone(),
                    self.service_repository.clone(),
                );
            });
            if self.service_repository.all_finished() {
                debug!("Result: {:?}", self.service_repository.services);
                debug!("All services have finished, exiting...");
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        std::process::exit(0);
    }

    /**
    Send a kill signal to all the services in the "Running" state.
    **/
    pub fn stop_all_services(&mut self) {
        self.service_repository.mutate_service_status(|sh| {
            if sh.is_running() && sh.pid().is_some() {
                debug!("Going to send SIGTERM signal to pid {:?}", sh.pid());
                // TODO: It might happen that we try to kill something which in the meanwhile has exited.
                // Thus here we should handle Error: Sys(ESRCH)
                kill(sh.pid().unwrap(), sh.service().termination.signal.into())
                    .map_err(|err| eprintln!("Error: {:?}", err))
                    .unwrap();
                sh.set_status(ServiceStatus::InKilling);
                return Some(sh);
            }
            if sh.is_initial() {
                debug!(
                    "Never going to run {}, so setting it to finished.",
                    sh.name()
                );
                sh.set_status(ServiceStatus::Finished);
                return Some(sh);
            }
            None
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
                service_repository.update_status(service.name.as_ref(), ServiceStatus::Failed);
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
    let chunks: Vec<String> = shlex::split(service.command.as_ref()).unwrap();
    let program_name = CString::new(chunks.get(0).unwrap().as_str()).unwrap();
    let arg_cstrings = chunks
        .into_iter()
        .map(|arg| CString::new(arg).map_err(Into::into))
        .collect::<Result<Vec<_>>>()
        .unwrap();
    //arg_cstrings.insert(0, program_name.clone());
    debug!("args: {:?}", arg_cstrings);
    let arg_cptr: Vec<&CStr> = arg_cstrings.iter().map(|c| c.as_c_str()).collect();
    // TODO: clear signal mask if needed.
    nix::unistd::execvp(program_name.as_ref(), arg_cptr.as_ref()).expect("Execvp() failed: ");
}
