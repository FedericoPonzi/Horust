use crate::error::HorustError;
use libc::{_exit, STDOUT_FILENO};
use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, SIGCHLD};
use nix::sys::wait::waitpid;
use nix::unistd::{chdir, execve, execvp, getpid, Pid};
use nix::unistd::{fork, getppid, ForkResult};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::ffi::{c_void, OsStr};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;
use std::{fs, io};
use structopt::StructOpt;

type ServiceName = String;

#[derive(Debug)]
pub struct Horust {
    services: Vec<Vec<Service>>,
    running: HashMap<ServiceName, (Pid, Service)>,
}
impl Horust {
    pub fn new(services: Vec<Service>) -> super::error::Result<Self> {
        super::runtime::topological_sort(services).map(|exec_order| {
            println!("Exec order: {:?}", exec_order);
            Horust {
                services: exec_order,
                running: HashMap::new(),
            }
        })
    }
    pub fn run(&self) -> super::error::Result<()> {
        self.services.into_iter().for_each(|service| {
            // \. Fork
            // 2. Save status, as in Started if needed,
            // 3. readiness check if needed
            // 4. continue looping.
        });
        Ok(())
    }

    pub fn from_services_dir<P>(path: &P) -> super::error::Result<Horust>
    where
        P: AsRef<Path> + ?Sized + AsRef<OsStr>,
    {
        Self::fetch_services(path)
            .map_err(Into::into)
            .and_then(|servs| Horust::new(servs))
    }

    pub fn fetch_services<P>(path: &P) -> io::Result<Vec<Service>>
    where
        P: AsRef<Path> + ?Sized + AsRef<OsStr>,
    {
        fs::read_dir(path).map(|dir| {
            dir.into_iter()
                .filter_map(std::result::Result::ok)
                .map(|direntry| direntry.path())
                .filter(|path| path.is_file())
                .map(|path| toml::from_str(fs::read_to_string(path).unwrap().as_str()).unwrap())
                .collect::<Vec<Service>>()
        })
    }

    pub fn spawn_process() {
        match fork() {
            Ok(ForkResult::Child) => {
                println!("Child PID: {}, PPID: {}.", getpid(), getppid());
                exit(123);
            }

            Ok(ForkResult::Parent { child, .. }) => {
                println!("Spawned child with PID {}.", child);
            }

            Err(err) => {
                panic!("fork() failed: {}", err);
            }
        };
    }

    fn setup_signal_handling(&self) {
        let sig_action = SigAction::new(
            SigHandler::Handler(Horust::handle_sigchld),
            SaFlags::empty(),
            SigSet::empty(),
        );

        if let Err(err) = unsafe { sigaction(SIGCHLD, &sig_action) } {
            panic!("[main] sigaction() failed: {}", err);
        };
    }
    extern "C" fn handle_sigchld(_: libc::c_int) {
        Horust::print_signal_safe("Received SIGCHILD.\n");
        match waitpid(Pid::from_raw(-1), None) {
            Ok(exit) => {
                // exit: WaitStatus has pid and exit code.
                Horust::print_signal_safe(format!("Child exited: {:?}.\n", exit).as_ref());
                Horust::exit_signal_safe(0);
            }
            Err(_) => {
                Horust::print_signal_safe("waitpid() failed.\n");
                Horust::exit_signal_safe(1);
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
}

struct ServiceIstance {
    service: Service,
    pid: Pid,
    status: ServiceStatus,
}
enum ServiceStatus {
    Stop,
    Running,
    Failed,
}

#[derive(Serialize, Clone, Deserialize, Debug)]
pub enum RestartStrategy {
    Always,
    OnFailure,
    Never,
}
impl Default for RestartStrategy {
    fn default() -> Self {
        RestartStrategy::Never
    }
}

#[derive(Serialize, Clone, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct Service {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub working_directory: PathBuf,
    #[serde(default)]
    pub start_delay: Duration,
    #[serde(default = "Vec::new")]
    pub start_after: Vec<ServiceName>,
    #[serde(default)]
    pub restart: RestartStrategy,
    #[serde(default)]
    pub restart_backoff: Duration,
}

impl Service {
    pub fn new(command: String) -> Self {
        Service {
            name: "Myservice".to_string(),
            command,
            working_directory: "".into(),
            start_after: vec![],
            start_delay: Default::default(),
            restart: RestartStrategy::Never,
            restart_backoff: Default::default(),
        }
    }
}
impl std::cmp::PartialEq for Service {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.command == other.command
    }
}
impl std::cmp::Eq for Service {}

fn default_duration() -> Duration {
    Duration::from_secs(0)
}
