use crate::horust::bus::BusConnector;
use crate::horust::error::Result;
use crate::horust::formats::{Event, LogOutput, Service};
use crate::horust::signal_safe::ss_panic;
use crossbeam::{after, tick};
use nix::fcntl;
use nix::unistd;
use nix::unistd::{fork, ForkResult, Pid};
use std::ffi::{CStr, CString};
use std::io;
use std::ops::Add;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::time::Duration;

/// Run another thread that will wait for the start delay and handle the fork / exec
pub(crate) fn spawn_fork_exec_handler(
    service: Service,
    backoff: Duration,
    bus: BusConnector<Event>,
) {
    std::thread::spawn(move || {
        let total_sleep = service.start_delay.clone().add(backoff);
        let timeout = after(total_sleep);
        let ticker = tick(Duration::from_millis(100));
        debug!("going to sleep: {:?}", total_sleep);
        // If start-delay is very high, this might interfere with the shutdown of the system.
        // the thread will listen for shutdown events from the bus, and will early exit if there is
        // a shuttingdowninitiated event
        let is_shutting_down_ev = |ev: Event| Event::ShuttingDownInitiated == ev;

        let ev = loop {
            select! {
                    recv(ticker) -> _ => {
                        let is_shutting_down = bus.try_get_events().into_iter().any(is_shutting_down_ev);
                        if is_shutting_down {
                            break Event::SpawnFailed(service.name.clone());
                        }
                    },
                    recv(timeout) -> _ => break match spawn_process(&service) {
                            Ok(pid) => {
                                debug!("Setting pid:{} for service: {}", pid, service.name);
                                Event::new_pid_changed(service.name.clone(), pid)
                            }
                            Err(error) => {
                                error!("Failed spawning the process: {}", error);
                                Event::SpawnFailed(service.name)
                            }
                    },
            }
        };
        bus.send_event(ev);
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
    let environment = service.get_environment()?;
    let env_cstrings = to_cstring(environment)?;

    Ok((program_name, arg_cstrings, env_cstrings))
}

/// Fork the process
fn spawn_process(service: &Service) -> Result<Pid> {
    debug!("Spawning process for service: {}", service.name);
    let (program_name, arg_cstrings, env_cstrings) = exec_args(service)?;
    let uid = service.user.get_uid()?;
    let cwd = service.working_directory.clone();
    match fork() {
        Ok(ForkResult::Child) => {
            let res = redirect_output(&service.stdout, LogOutput::Stdout)
                .and_then(|_| redirect_output(&service.stderr, LogOutput::Stderr))
                .and_then(|_| exec(program_name, arg_cstrings, env_cstrings, uid, cwd));
            if let Err(error) = res {
                let error = format!("Error spawning process: {}", error);
                ss_panic(error.as_str(), 102);
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

fn redirect_output(val: &LogOutput, output: LogOutput) -> Result<()> {
    let stdout = io::stdout().as_raw_fd();
    let stderr = io::stderr().as_raw_fd();
    match val {
        // stderr = "STDOUT"
        LogOutput::Stdout if output == LogOutput::Stderr => {
            unistd::dup2(stdout, stderr)?;
        }
        // stdout = "STDERR"
        LogOutput::Stderr if output == LogOutput::Stdout => {
            // Redirect stdout to stderr
            unistd::dup2(stderr, stdout)?;
        }
        LogOutput::Path(path) => {
            let raw_fd = fcntl::open(
                path,
                fcntl::OFlag::O_CREAT | fcntl::OFlag::O_WRONLY | fcntl::OFlag::O_APPEND,
                nix::sys::stat::Mode::S_IRWXU,
            )?;
            if output == LogOutput::Stdout {
                unistd::dup2(raw_fd, stdout)?;
            } else {
                unistd::dup2(raw_fd, stderr)?;
            }
        }
        _ => (),
    };
    Ok(())
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
