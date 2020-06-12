use crate::horust::runtime::repo::Repo;
use crate::horust::Event;

/// Reaps up to 20 dead processes
pub(crate) fn run(repo: &Repo) -> Vec<Event> {
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    use nix::unistd::Pid;
    (0..20)
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
