use crate::horust::service_handler::{ServiceHandler, Services};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::time::Duration;

pub(crate) fn spawn(supervised: Services) {
    std::thread::spawn(|| {
        supervisor_thread(supervised);
    });
}
/// A endlessly running function meant to be run in a separate thread.
/// Its purpose is to continuously try to reap possibly dead children.
pub(crate) fn supervisor_thread(supervised: Services) {
    let mut reapable = HashMap::new();
    loop {
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
            let mut locked = supervised.0.lock().unwrap();
            debug!("pid:{:?}, locked: {:?}", pid, locked);
            let service: Option<&mut ServiceHandler> = locked
                .iter_mut()
                .skip_while(|sh| sh.pid() != Some(pid))
                .take(1)
                .last();
            if let Some(service) = service {
                service.set_status_by_exit_code(*exit_code);
                return true;
            }
            // If is a grandchildren, we don't care about it:
            // is grandchildren =
            locked.iter().any(|sh| sh.is_to_be_run())
        });
        std::thread::sleep(Duration::from_millis(500))
    }
}
