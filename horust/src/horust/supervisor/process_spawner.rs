use anyhow::{anyhow, Context, Result};
use crossbeam::channel::{after, tick};
use nix::errno::Errno;
use nix::fcntl;
use nix::unistd;
use nix::unistd::{fork, ForkResult, Pid, Uid};
use std::ffi::{CStr, CString};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs::File, io::BufReader};
use std::{fs::OpenOptions, ops::Add};
use std::{
    io::{self, Read},
    os::fd::OwnedFd,
};

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
    let program_name = String::from(chunks.first().unwrap());
    let to_cstring = |s: Vec<String>| {
        s.into_iter()
            .map(|arg| CString::new(arg).map_err(Into::into))
            .collect::<Result<Vec<_>>>()
    };
    let arg_cstrings = to_cstring(chunks)?;
    let environment = service.get_environment()?;
    let env_cstrings = to_cstring(environment)?;
    let path = if program_name.contains('/') {
        program_name.to_string()
    } else {
        find_program(&program_name)?
    };
    Ok((CString::new(path)?, arg_cstrings, env_cstrings))
}

#[inline]
fn child_process_main(
    service: &Service,
    path: CString,
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
    if let Err(errno) = exec(path, arg_cptr, env_cptr, uid, cwd) {
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
    let (path, arg_cstrings, env_cstrings) = exec_args(service)?;
    let uid = service.user.get_uid()?;
    let cwd = service.working_directory.clone();
    let arg_cptr: Vec<&CStr> = arg_cstrings.iter().map(|c| c.as_c_str()).collect();
    let env_cptr: Vec<&CStr> = env_cstrings.iter().map(|c| c.as_c_str()).collect();
    let mut service_copy = service.clone();
    let (pipe_read, pipe_write) = if service.stdout_rotate_size > 0 {
        let (pipe_read, pipe_write) = unistd::pipe()?;
        (Some(pipe_read), Some(pipe_write))
    } else {
        (None, None)
    };
    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            if let Some(pipe_write) = &pipe_write {
                drop(pipe_read.unwrap());
                service_copy.stdout = LogOutput::Pipe(pipe_write.as_raw_fd());
            }
            child_process_main(&service_copy, path, cwd, uid, arg_cptr, env_cptr);
            unreachable!();
            // Here the "pipe_write" would go out of scope and its descriptor would be closed.
            // But because child_process_main() does an exec() and never returns, the raw
            // descriptor inside the LogOutput::Pipe stays open.
        }
        Ok(ForkResult::Parent { child, .. }) => {
            pipe_read.and_then(|pipe| {
                drop(pipe_write.unwrap());
                std::thread::spawn(move || {
                    chunked_writer(pipe, service_copy).map_err(|e| error!("{e}"))
                });
                None::<()>
            });
            // only the root user and authorized users can manage the cgroup
            if let Err(err) = service.resource_limit.apply(&service.name, child) {
                warn!(
                    "Failed to add the resource limit to {}: {}",
                    &service.name, err
                );
            }
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
        (LogOutput::Pipe(pipe), LogOutput::Stderr) => {
            unistd::dup2(*pipe, stderr)?;
        }
        (LogOutput::Pipe(pipe), LogOutput::Stdout) => {
            unistd::dup2(*pipe, stdout)?;
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

fn open_next_chunk(
    base_path: &Path,
    timestamp: u64,
    stdout_should_append_timestamp_to_filename: bool,
    count: u32,
) -> io::Result<File> {
    let filename = match (stdout_should_append_timestamp_to_filename, count > 0) {
        (true, true) => format!("{}.{timestamp}.{count}", base_path.to_string_lossy()),
        (true, false) => format!("{}.{timestamp}", base_path.to_string_lossy()),
        (false, true) => format!("{}.{count}", base_path.to_string_lossy()),
        (false, false) => base_path.to_string_lossy().to_string(),
    };
    let path = PathBuf::from(&filename);

    debug!("Opening next log output: {}", path.display());
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
}

fn chunked_writer(fd: OwnedFd, service: Service) -> Result<()> {
    let source = File::from(fd);
    let path = match &service.stdout {
        LogOutput::Path(path) => path,
        _ => return Err(anyhow!("Log output path is not set")),
    };
    let mut chunk = 0;
    let mut reader = BufReader::new(&source);
    // Get the current Unix timestamp
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();
    loop {
        let mut capped = (&mut reader).take(service.stdout_rotate_size);
        let mut output = open_next_chunk(
            path,
            timestamp,
            service.stdout_should_append_timestamp_to_filename,
            chunk,
        )?;
        chunk += 1;
        let copied = io::copy(&mut capped, &mut output)?;
        if copied < service.stdout_rotate_size {
            debug!("EOF reached");
            break;
        }
    }
    Ok(())
}

/// Find program on PATH.
///
pub(crate) fn find_program(program_name: &String) -> Result<String> {
    let path_var = match std::env::var_os("PATH") {
        Some(val) => val,
        None => return Err(anyhow!("PATH environment variable is not set")),
    };

    let paths: Vec<PathBuf> = std::env::split_paths(&path_var).collect();

    for path in paths {
        let program_path = path.join(program_name);

        // Check if the program file exists at this path
        if program_path.is_file() {
            return Ok(program_path.into_os_string().into_string().unwrap());
        }
    }

    Err(anyhow!(
        "Program {:?} not found in any of the PATH directories",
        program_name
    ))
}

/// Exec wrapper.
///
/// # Safety
///
/// Use only async-signal-safe, otherwise it might lock.
#[inline]
fn exec(
    path: CString,
    arg_cptr: Vec<&CStr>,
    env_cptr: Vec<&CStr>,
    uid: Uid,
    cwd: PathBuf,
) -> std::result::Result<(), Errno> {
    // Changes the current working directory to the specified path.
    unistd::chdir(&cwd)?;
    // Create new session and set process group id
    unistd::setsid()?;
    // Set the user ID
    unistd::setuid(uid)?;
    unistd::execve(path.as_ref(), arg_cptr.as_ref(), env_cptr.as_ref())?;
    Ok(())
}
