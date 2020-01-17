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

pub type ServiceName = String;

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

#[cfg(test)]
mod test {
    use crate::formats::{RestartStrategy, Service};
    use std::time::Duration;

    impl Service {
        pub fn start_after(name: &str, start_after: Vec<&str>) -> Self {
            Service {
                name: name.to_owned(),
                start_after: start_after.into_iter().map(|v| v.into()).collect(),
                working_directory: "".into(),
                restart: RestartStrategy::Always,
                start_delay: Duration::from_secs(0),
                command: "".to_string(),
                restart_backoff: Default::default(),
            }
        }
        pub fn from_name(name: &str) -> Self {
            Self::start_after(name, Vec::new())
        }
    }
}
