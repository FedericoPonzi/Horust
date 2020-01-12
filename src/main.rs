mod error;
mod formats;
mod runtime;

use crate::formats::{RestartStrategy, Service};
use libc::{_exit, STDOUT_FILENO};
use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, SIGCHLD};
use nix::sys::wait::waitpid;
use nix::unistd::{chdir, execve, execvp, getpid, Pid};
use nix::unistd::{fork, getppid, ForkResult};
use std::env::args;
use std::ffi::c_void;
use std::path::Path;
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;
use std::{fs, io};
use structopt::StructOpt;

extern "C" fn handle_sigchld(_: libc::c_int) {
    print_signal_safe("[main] What a surprise! Got SIGCHLD!\n");
    match waitpid(Pid::from_raw(-1), None) {
        Ok(exit) => {
            // exit: WaitStatus has pid and exit code.
            print_signal_safe(format!("[main] Child exited with code: {:?}.\n", exit).as_ref());
            print_signal_safe("[main] Bye Bye!\n");
            exit_signal_safe(0);
        }
        Err(_) => {
            print_signal_safe("[main] waitpid() failed.\n");
            exit_signal_safe(1);
        }
    }
}

fn print_signal_safe(s: &str) {
    unsafe {
        libc::write(STDOUT_FILENO, s.as_ptr() as (*const c_void), s.len());
    }
}

fn exit_signal_safe(status: i32) {
    unsafe {
        _exit(status);
    }
}

fn test_signals() {
    println!("[main] Hi there! My PID is {}.", getpid());

    match fork() {
        Ok(ForkResult::Child) => {
            println!(
                "[child] I'm alive! My PID is {} and PPID is {}.",
                getpid(),
                getppid()
            );

            println!("[child] I'm gonna sleep for a while and then just exit...");
            sleep(Duration::from_secs(2));
            println!("Adieu!");
            exit(123);
        }

        Ok(ForkResult::Parent { child, .. }) => {
            println!("[main] I forked a child with PID {}.", child);
        }

        Err(err) => {
            panic!("[main] fork() failed: {}", err);
        }
    };

    let sig_action = SigAction::new(
        SigHandler::Handler(handle_sigchld),
        SaFlags::empty(),
        SigSet::empty(),
    );

    if let Err(err) = unsafe { sigaction(SIGCHLD, &sig_action) } {
        panic!("[main] sigaction() failed: {}", err);
    };

    println!("[main] I'll be doing my own stuff...");
    loop {
        println!("[main] Do my own stuff.");
        // ... replace sleep with the payload
        sleep(Duration::from_millis(500));
    }
}
pub fn load_service(l: Service) {}
pub fn run_program(c: String) {}
pub fn fetch_services(dir: &Path) -> io::Result<Vec<Service>> {
    Ok(fs::read_dir(dir)?
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                println!("Not a dir!");
            };
            Service::new(path)
        })
        .collect::<Vec<Service>>())
}

#[derive(StructOpt, Debug)]
#[structopt(version = "0.1", author = "Federico Ponzi", name = "horust")]
struct Opts {
    /// Sets a custom config file. Could have been an Option<T> with no default too
    #[structopt(short = "c", long = "config", default_value = "default.conf")]
    config: String,
    /// Some input. Because this isn't an Option<T> it's required to be used
    input: String,
    #[structopt(short, long, parse(from_occurrences))]
    /// A level of verbosity, and can be used multiple times
    verbose: i32,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    /*
    if (getpid() != 1) {
        std::process::exit(1);
    }
    chdir("/");
    */
    // execvp()
    //1. Fetch services:
    let path = Path::new("/etc/horust/services");
    let services: Vec<Service> = fetch_services(&path)?;
    //2. Start all of them:
    //3. Start
    println!("Size: {}, {:?}", services.len(), services);
    //test_signals();

    let opt = Opts::from_args();
    println!("{:#?}", opt);

    Ok(())
}
