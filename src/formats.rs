use nix::unistd::Pid;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

type ServiceName = String;

#[derive(Debug)]
pub struct Horus {
    services: HashMap<ServiceName, (Pid, Service)>,
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
    // TODO: Change to program + args.
    pub path: PathBuf,
    #[serde(default)]
    pub restart: RestartStrategy,
    #[serde(default)]
    pub start_delay: Duration,
    #[serde(default = "Vec::new")]
    pub start_after: Vec<ServiceName>,
}

impl Service {
    pub fn new(path: PathBuf) -> Self {
        Service {
            name: "Myservice".to_string(),
            path,
            restart: Default::default(),
            start_after: vec![],
            start_delay: Default::default(),
        }
    }
}
impl std::cmp::PartialEq for Service {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.path == other.path
    }
}
impl std::cmp::Eq for Service {}

fn default_duration() -> Duration {
    Duration::from_secs(0)
}
