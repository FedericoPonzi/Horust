use crate::horust::runtime::repo::Repo;
use crate::horust::Event;

/// Reaps up to `max_iterations` dead processes
pub(crate) fn run(repo: &Repo, max_iterations: u32) -> Vec<Event> {
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    use nix::unistd::Pid;
    (0..max_iterations)
        .filter_map(
            |_| match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
                Ok(wait_status) => {
                    if let WaitStatus::Exited(pid, exit_code) = wait_status {
                        debug!("Pid has exited: {} with exitcode: {}", pid, exit_code);
                        repo.get_service_by_pid(pid)
                            .map(|s_name| (s_name, exit_code))
                    } else {
                        None
                    }
                }
                Err(err) => {
                    if !err.to_string().contains("ECHILD") {
                        error!("Error waitpid(): {}", err);
                    }
                    None
                }
            },
        )
        .map(|(sname, exit_code)| {
            debug!("Service '{:?}' has exited.", sname);
            Event::new_service_exited(sname.into(), exit_code)
        })
        .collect()
}
