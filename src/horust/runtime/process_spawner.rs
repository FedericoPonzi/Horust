use crate::horust::error::Result;
use crate::horust::formats::{Event, Service, ServiceStatus};
use crate::horust::runtime::Repo;
use nix::unistd;
use nix::unistd::{fork, ForkResult, Pid};
use shlex;
use std::ffi::{CStr, CString};
use std::ops::Add;
use std::path::PathBuf;
use std::time::Duration;

/// Run another thread that will wait for the start delay and handle the fork / exec
pub(crate) fn spawn_fork_exec_handler(service: Service, backoff: Duration, mut repo: Repo) {
    std::thread::spawn(move || {
        // todo: we should wake up every second, in case someone wants to kill this process.
        debug!("going to sleep: {:?}", service.start_delay.add(backoff));
        std::thread::sleep(service.start_delay.add(backoff));
        let evs = match spawn_process(&service) {
            Ok(pid) => {
                debug!("Setting pid:{} for service: {}", pid, service.name);
                vec![
                    Event::new_pid_changed(service.name.clone(), pid),
                    Event::new_status_changed(&service.name, ServiceStatus::Started),
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

/// Creates the execvpe arguments out of a Service
fn exec_args(service: &Service) -> Result<(CString, Vec<CString>, Vec<CString>)> {
    let chunks: Vec<String> = shlex::split(service.command.as_ref()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Invalid command: {}", service.command,),
        )
    })?;
    let program_name = CString::new(chunks.get(0).unwrap().as_str())?;
    let to_cstring = |s: Vec<String>| {
        s.into_iter()
            .map(|arg| CString::new(arg).map_err(Into::into))
            .collect::<Result<Vec<_>>>()
    };
    let arg_cstrings = to_cstring(chunks)?;
    let environment = service.get_environment();
    let env_cstrings = to_cstring(environment)?;

    Ok((program_name, arg_cstrings, env_cstrings))
}

/// Fork the process
fn spawn_process(service: &Service) -> Result<Pid> {
    let (program_name, arg_cstrings, env_cstrings) = exec_args(service)?;
    let uid = service.user.get_uid();
    let cwd = service.working_directory.clone();
    match fork() {
        Ok(ForkResult::Child) => {
            if let Err(error) = exec(program_name, arg_cstrings, env_cstrings, uid, cwd) {
                let error = format!("Error spawning process: {}", error);
                eprint_safe(error.as_str());
                exit_safe(102);
            }
            unreachable!()
        }
        Ok(ForkResult::Parent { child, .. }) => {
            debug!("Spawned child with PID {}.", child);
            Ok(child)
        }
        Err(err) => Err(Into::into(err)),
    }
}

/// Exec wrapper.
/// Warning: use only async-signal-safe, otherwise it might lock
fn exec(
    program_name: CString,
    arg_cstrings: Vec<CString>,
    env_cstrings: Vec<CString>,
    uid: unistd::Uid,
    cwd: PathBuf,
) -> Result<()> {
    let arg_cptr: Vec<&CStr> = arg_cstrings.iter().map(|c| c.as_c_str()).collect();
    let env_cptr: Vec<&CStr> = env_cstrings.iter().map(|c| c.as_c_str()).collect();
    // Changes the current working directory to the specified path.
    std::env::set_current_dir(cwd)?;
    // Create new session and set process group id
    nix::unistd::setsid()?;
    // Set the user ID
    nix::unistd::setuid(uid)?;
    nix::unistd::execvpe(program_name.as_ref(), arg_cptr.as_ref(), env_cptr.as_ref())?;
    Ok(())
}

/// Async-signal-safe stderr print
fn eprint_safe(s: &str) {
    use libc::{write, STDERR_FILENO};
    use std::ffi::c_void;
    unsafe {
        write(STDERR_FILENO, s.as_ptr() as *const c_void, s.len());
        let s = "\n";
        write(STDERR_FILENO, s.as_ptr() as *const c_void, s.len());
    }
}

/// Async-signal-safe exit
fn exit_safe(status: i32) {
    use libc::_exit;
    unsafe {
        _exit(status);
    }
}
