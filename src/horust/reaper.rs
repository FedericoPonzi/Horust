use crate::horust::repository::ServiceRepository;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::time::Duration;

pub(crate) fn spawn(service_repository: ServiceRepository) {
    std::thread::spawn(move || {
        supervisor_thread(service_repository);
    });
}
/// A endlessly running function meant to be run in a separate thread.
/// Its purpose is to continuously try to reap possibly dead children.
pub(crate) fn supervisor_thread(mut service_repository: ServiceRepository) {
    let mut reapable = HashMap::new();
    loop {
        service_repository.ingest("reaper");
        match waitpid(Pid::from_raw(-1), None) {
            Ok(wait_status) => {
                if let WaitStatus::Exited(pid, exit_code) = wait_status {
                    debug!("Pid has exited: {} with exitcode: {}", pid, exit_code);
                    reapable.insert(pid, exit_code);
                }
            }
            Err(err) => {
                if !err.to_string().contains("ECHILD") {
                    error!("Error waitpid(): {}", err);
                }
            }
        }
        // It might happen that before supervised was updated, the process was already started, executed,
        // and exited. Thus we're trying to reaping it, but there is still no map Pid -> Service.
        reapable.retain(|pid, exit_code| {
            let result = service_repository.update_status_by_exit_code(*pid, *exit_code);
            // If is a grandchildren, we don't care about it:
            // is grandchildren =
            result || service_repository.is_any_service_to_be_run()
        });
        std::thread::sleep(Duration::from_millis(500))
    }
}
