use std::ffi::{CStr, CString};
use std::io;
use std::ops::Add;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam::channel::{after, tick};
use nix::errno::Errno;
use nix::fcntl;
use nix::unistd;
use nix::unistd::{fork, ForkResult, Pid, Uid};

use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, LogOutput, Service};
use crate::horust::signal_safe::panic_ssafe;

/// Run another thread that will wait for the start delay and handle the fork / exec
pub(crate) fn spawn_fork_exec_handler(
    service: Service,
    backoff: Duration,
    bus: BusConnector<Event>,
) {
    std::thread::spawn(move || {
        let total_sleep = service.start_delay.add(backoff);
        let timeout = after(total_sleep);
        let ticker = tick(Duration::from_millis(100));
        debug!("going to sleep: {:?}", total_sleep);
        // If start-delay is very high, this might interfere with the shutdown of the system.
        // the thread will listen for shutdown events from the bus, and will early exit if there is
        // a shuttingdowninitiated event
        let is_shutting_down_ev = |ev: Event| matches!(ev, Event::ShuttingDownInitiated(_));

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

/// Produces the execvpe arguments out of a `Service`
#[inline]
fn exec_args(service: &Service) -> Result<(CString, Vec<CString>, Vec<CString>)> {
    let chunks: Vec<String> =
        shlex::split(&service.command).context(format!("Invalid command: {}", service.command,))?;
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

#[inline]
fn child_process_main(
    service: &Service,
    program_name: CString,
    cwd: PathBuf,
    uid: Uid,
    arg_cptr: Vec<&CStr>,
    env_cptr: Vec<&CStr>,
) {
    if let Err(errno) = redirect_output(&service.stdout, LogOutput::Stdout) {
        panic_ssafe(
            "child_process_main: Redirect stdout failed.",
            Some(&service.name),
            errno,
            101,
        );
    }
    if let Err(errno) = redirect_output(&service.stderr, LogOutput::Stderr) {
        panic_ssafe(
            "child_process_main: Redirect stderr failed.",
            Some(&service.name),
            errno,
            102,
        );
    }
    if let Err(errno) = exec(program_name, arg_cptr, env_cptr, uid, cwd) {
        panic_ssafe(
            "child_process_main: Failed to exec the new process.",
            Some(&service.name),
            errno,
            103,
        );
    }
}

/// Fork the process
fn spawn_process(service: &Service) -> Result<Pid> {
    debug!("Spawning process for service: {}", service.name);
    let (program_name, arg_cstrings, env_cstrings) = exec_args(service)?;
    let uid = service.user.get_uid()?;
    let cwd = service.working_directory.clone();
    let arg_cptr: Vec<&CStr> = arg_cstrings.iter().map(|c| c.as_c_str()).collect();
    let env_cptr: Vec<&CStr> = env_cstrings.iter().map(|c| c.as_c_str()).collect();
    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            child_process_main(service, program_name, cwd, uid, arg_cptr, env_cptr);
            unreachable!();
        }
        Ok(ForkResult::Parent { child, .. }) => {
            debug!("Spawned child with PID {}.", child);
            Ok(child)
        }
        Err(err) => Err(Into::into(err)),
    }
}

/// Sets up the stdout / stderr descriptors.
fn redirect_output(
    target_stream: &LogOutput,
    into_output_stream: LogOutput,
) -> std::result::Result<(), Errno> {
    let stdout = io::stdout().as_raw_fd();
    let stderr = io::stderr().as_raw_fd();
    match (target_stream, into_output_stream) {
        // stderr = "STDOUT"
        (LogOutput::Stdout, LogOutput::Stderr) => {
            unistd::dup2(stdout, stderr)?;
        }
        // stdout = "STDERR"
        (LogOutput::Stderr, LogOutput::Stdout) => {
            // Redirect stdout to stderr
            unistd::dup2(stderr, stdout)?;
        }
        (LogOutput::Path(path), LogOutput::Stdout) => {
            let raw_fd = fcntl::open(
                path,
                fcntl::OFlag::O_CREAT | fcntl::OFlag::O_WRONLY | fcntl::OFlag::O_APPEND,
                nix::sys::stat::Mode::S_IRWXU,
            )?;
            unistd::dup2(raw_fd, stdout)?;
        }
        (LogOutput::Path(path), LogOutput::Stderr) => {
            let raw_fd = fcntl::open(
                path,
                fcntl::OFlag::O_CREAT | fcntl::OFlag::O_WRONLY | fcntl::OFlag::O_APPEND,
                nix::sys::stat::Mode::S_IRWXU,
            )?;
            unistd::dup2(raw_fd, stderr)?;
        }
        // Should never happen.
        _ => (),
    };
    Ok(())
}

/// Exec wrapper.
///
/// # Safety
///
/// Use only async-signal-safe, otherwise it might lock.
#[inline]
fn exec(
    program_name: CString,
    arg_cptr: Vec<&CStr>,
    env_cptr: Vec<&CStr>,
    uid: unistd::Uid,
    cwd: PathBuf,
) -> std::result::Result<(), Errno> {
    // Changes the current working directory to the specified path.
    nix::unistd::chdir(&cwd)?;
    // Create new session and set process group id
    nix::unistd::setsid()?;
    // Set the user ID
    nix::unistd::setuid(uid)?;
    nix::unistd::execvpe(program_name.as_ref(), arg_cptr.as_ref(), env_cptr.as_ref())?;
    Ok(())
}
