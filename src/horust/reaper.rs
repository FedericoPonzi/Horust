use crate::horust::formats::{RestartStrategy, ServiceStatus};
use crate::horust::ServiceHandler;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A endlessly running function meant to be run in a separate thread.
/// Its purpose is to continuously try to reap possibly dead children.
pub(crate) fn supervisor_thread(supervised: Arc<Mutex<Vec<ServiceHandler>>>) {
    let mut reapable = HashMap::new();
    loop {
        match waitpid(Pid::from_raw(-1), None) {
            Ok(wait_status) => {
                if let WaitStatus::Exited(pid, exit_code) = wait_status {
                    debug!("Pid has exited: {}", pid);
                    reapable.insert(pid, exit_code);
                    reapable = reapable
                        .into_iter()
                        .filter_map(|(pid, exit_code)| {
                            let mut locked = supervised.lock().unwrap();
                            debug!("{:?}", locked);
                            let service: Option<&mut ServiceHandler> = locked
                                .iter_mut()
                                .filter(|sh| sh.pid == Some(pid))
                                .take(1)
                                .last();
                            // It might happen that before supervised was updated, the process was already started, executed,
                            // and exited. Thus we're trying to reaping it, but there is still no map Pid -> Service.
                            if let Some(service) = service {
                                // TODO: Restart strategy
                                match service.restart() {
                                    RestartStrategy::Never => {
                                        debug!("Pid successfully exited.");
                                        service.status = ServiceStatus::from_exit(exit_code);
                                        debug!("new locked: {:?}", locked);
                                    }
                                    RestartStrategy::OnFailure => {
                                        service.status = ServiceStatus::from_exit(exit_code);
                                        debug!("Going to rerun the process because it failed!");
                                    }
                                    RestartStrategy::Always => {
                                        service.status = ServiceStatus::Stopped;
                                    }
                                }
                                return None;
                            }
                            Some((pid, exit_code))
                        })
                        .collect();
                }
            }
            Err(err) => {
                if !err.to_string().contains("ECHILD") {
                    error!("Error waitpid(): {}", err);
                }
            }
        }
        std::thread::sleep(Duration::from_secs(1))
    }
}
