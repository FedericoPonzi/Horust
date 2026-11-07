//! Reaper module
//! This module is concerned with process reaping. After a process has exited, the reaper will
//! collect its exit status. This module will make sure to reap also zombie processes and not only
//! child services.

use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

use crate::horust::Event;
use crate::horust::supervisor::repo::Repo;

/// Reaps up to `max_iterations` dead processes
pub(crate) fn run(repo: &Repo, max_iterations: u32) -> Vec<Event> {
    (0..max_iterations)
        .filter_map(
            |_| {
                let result = waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG));
                #[cfg(feature = "formal-trace")]
                crate::horust::formal_trace::record("wait", serde_json::json!({"status": format!("{result:?}")}));
                match result {
                Ok(wait_status) => match wait_status {
                    WaitStatus::Exited(pid, exit_code) => {
                        #[cfg(feature = "formal-trace")]
                        crate::horust::formal_trace::reap(pid.as_raw(), exit_code, repo.get_service_by_pid(pid).map(String::as_str));
                        debug!("Pid '{}' has exited with status: {}", pid, exit_code);
                        repo.get_service_by_pid(pid)
                            .map(|s_name| (s_name, exit_code))
                    }
                    WaitStatus::Signaled(pid, signal, core_dumped) => {
                        #[cfg(feature = "formal-trace")]
                        {
                            crate::horust::formal_trace::record("wait_signal", serde_json::json!({
                                "pid": pid.as_raw(), "signal": signal as i32, "core_dumped": core_dumped,
                            }));
                            crate::horust::formal_trace::reap(pid.as_raw(), -137, repo.get_service_by_pid(pid).map(String::as_str));
                        }
                        debug!(
                            "Pid '{:?}' has exited due to signal: {:?}. Core dumped: {}",
                            pid, signal, core_dumped
                        );
                        repo.get_service_by_pid(pid).map(|s_name| (s_name, -137))
                    }
                    _ => None,
                },
                Err(err) => {
                    // waitpid() call can fail with:
                    // EINVAL: Options argument is not valid - should not apply.
                    // EINTR:  Possible & common.
                    // ECHILD: The process specified by pid does not exist or is not a child of the
                    // calling process, or the process group specified by pid does not exist or does
                    // not have any member process that is a child of the calling process.
                    // Since we're using -1 as parameter, we should never fall in this branch.
                    if !err.to_string().contains("ECHILD") {
                        // This is the only "actual" error.
                        error!("Error waitpid(): {}", err);
                    }
                    None
                }
                }
            },
        )
        .map(|(sname, exit_code)| {
            debug!("Service '{:?}' has exited.", sname);
            Event::new_service_exited(sname.into(), exit_code)
        })
        .collect()
}
