use crate::horust::service_handler::ServiceHandler;
use crate::horust::HorustError;
use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

pub type ServiceName = String;

#[derive(Debug, Clone)]
pub struct UpdatesQueue {
    pub sender: Sender<Event>,
    pub receiver: Receiver<Event>,
}
impl UpdatesQueue {
    pub fn new(s: Sender<Event>, r: Receiver<Event>) -> Self {
        UpdatesQueue {
            sender: s,
            receiver: r,
        }
    }
    fn send_update(&self, ev: Event) {
        debug!("Going to send the following event: {:?}", ev);
        self.sender.send(ev).expect("Failed sending update event!");
    }
    pub fn send_update_pid(&self, sh: &ServiceHandler) {
        self.send_update(Event::new(sh.clone(), EventKind::PidChanged));
    }
    pub fn send_updated_status(&self, sh: &ServiceHandler) {
        self.send_update(Event::new(sh.clone(), EventKind::StatusChanged));
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub(crate) service_handler: ServiceHandler,
    pub(crate) kind: EventKind,
}
impl Event {
    pub fn new(service_handler: ServiceHandler, kind: EventKind) -> Self {
        Event {
            service_handler,
            kind,
        }
    }
}
#[derive(Debug, Clone)]
pub enum EventKind {
    StatusChanged,
    PidChanged,
    //ServiceCreated(ServiceHandler),
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Service {
    #[serde(default)]
    pub name: String,
    #[serde()]
    pub command: String,
    #[serde()]
    pub working_directory: Option<PathBuf>,
    #[serde(default, with = "humantime_serde")]
    pub start_delay: Duration,
    #[serde(default = "Vec::new")]
    pub start_after: Vec<ServiceName>,
    #[serde(default)]
    pub restart: Restart,
    #[serde()]
    pub healthiness: Option<Healthness>,
    #[serde()]
    pub signal_rewrite: Option<String>,
    #[serde(skip)]
    pub last_mtime_sec: i64,
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Healthness {
    pub http_endpoint: Option<String>,
    pub file_path: Option<PathBuf>,
}

pub fn get_sample_service() -> String {
    r#"name = "my-cool-service"
command = "/bin/bash -c 'echo hello world'"
working-directory = "/tmp/"
start-delay = "2s"
[restart]
strategy = "never"
backoff = "0s"
attempts = 0
[healthiness]
http_endpoint = "http://localhost:8080/healthcheck"
file_path = "/var/myservice/up""#
        .to_string()
}

impl Service {
    pub fn from_file(path: &PathBuf) -> Result<Self, HorustError> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str::<Service>(content.as_str()).map_err(HorustError::from)
    }

    pub fn from_command(command: String) -> Self {
        Service {
            name: command.clone(),
            start_after: Default::default(),
            working_directory: Some("/".into()),
            restart: Default::default(),
            start_delay: Duration::from_secs(0),
            command,
            healthiness: None,
            signal_rewrite: None,
            last_mtime_sec: 0,
        }
    }
}

impl FromStr for Service {
    type Err = HorustError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        toml::from_str::<Service>(s).map_err(HorustError::from)
    }
}

/// Visualize: https://state-machine-cat.js.org/
/*
initial => Initial : "Will eventually be run";
Initial => ToBeRun : "All dependencies are running, a thread has spawned and will run the fork/exec the process";
ToBeRun => Starting : "The ServiceHandler has a pid";
Starting => Running : "The service has met healthiness policy";
Starting => Failed : "Service cannot be started";
Running => Finished : "Exit status = 0";
Running => InKilling : "Shutdown request received";
InKilling => Finished : "Succesfully killed";
InKilling => Failed : "Forcefully killed (SIGKILL)";
Running => Failed  : "Exit status != 0";
Finished => Initial : "restart = Always";
Failed => Initial : "restart = always|on-failure";
*/

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
pub enum ServiceStatus {
    Starting,
    /// This is just an intermediate state between Initial and Running.
    ToBeRun,
    /// The service is up and healthy
    Running,
    /// Signal sent, waiting for the process to terminate.
    InKilling,
    /// A finished service has done it's job and won't be restarted.
    Finished,
    /// A Failed service might be restarted if the restart policy demands so.
    Failed,
    /// This is the initial state: A service in Initial state is marked to be runnable:
    /// it will be run as soon as possible.
    Initial,
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Restart {
    #[serde(default)]
    pub strategy: RestartStrategy,
    #[serde(default, with = "humantime_serde")]
    backoff: Duration,
    #[serde(default)]
    attempts: u32,
}

impl Default for Restart {
    fn default() -> Self {
        Restart {
            strategy: RestartStrategy::Never,
            backoff: Duration::from_secs(0),
            attempts: 0,
        }
    }
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
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

impl From<String> for RestartStrategy {
    fn from(strategy: String) -> Self {
        strategy.as_str().into()
    }
}

impl From<&str> for RestartStrategy {
    fn from(strategy: &str) -> Self {
        match strategy.to_lowercase().as_str() {
            "always" => RestartStrategy::Always,
            "on-failure" => RestartStrategy::OnFailure,
            "never" => RestartStrategy::Never,
            _ => RestartStrategy::Never,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::horust::formats::Service;
    use crate::horust::get_sample_service;
    use std::str::FromStr;
    use std::time::Duration;

    impl Service {
        pub fn start_after(name: &str, start_after: Vec<&str>) -> Self {
            Service {
                name: name.to_owned(),
                start_after: start_after.into_iter().map(|v| v.into()).collect(),
                working_directory: Some("".into()),
                restart: Default::default(),
                start_delay: Duration::from_secs(0),
                command: "".to_string(),
                healthiness: None,
                signal_rewrite: None,
                last_mtime_sec: 0,
            }
        }

        pub fn from_name(name: &str) -> Self {
            Self::start_after(name, Vec::new())
        }
    }
    #[test]
    fn test_should_correctly_deserialize_sample() {
        let service = Service::from_str(get_sample_service().as_str());
        assert!(service.is_ok());
    }
}
