use crate::horust::HorustError;
use nix::sys::signal::Signal;
use nix::sys::signal::{SIGHUP, SIGINT, SIGKILL, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2};
use nix::unistd;
use serde::export::fmt::Error;
use serde::export::Formatter;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

pub type ServiceName = String;

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Service {
    #[serde(default)]
    pub name: ServiceName,
    #[serde()]
    pub command: String,
    #[serde(default)]
    pub user: User,
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
    #[serde(default)]
    pub failure: Failure,
    #[serde(default)]
    pub termination: Termination,
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Healthness {
    pub http_endpoint: Option<String>,
    pub file_path: Option<PathBuf>,
}

pub fn get_sample_service() -> String {
    r#"# The name of your service, must be unique. It's optional, will use the filename as name.
name = "my-cool-service"
command = "/bin/bash -c 'echo hello world'"
working-directory = "/tmp/"
start-delay = "2s"
[restart]
strategy = "never"
backoff = "0s"
attempts = 0
[healthiness]
http_endpoint = "http://localhost:8080/healthcheck"
file_path = "/var/myservice/up"
[failure]
exit_code = [ 1, 2, 3]
strategy = "ignore"
[termination]
signal = "TERM"
wait = "10s"
"#
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
            user: Default::default(),
            working_directory: Some("/".into()),
            restart: Default::default(),
            start_delay: Duration::from_secs(0),
            command,
            healthiness: None,
            signal_rewrite: None,
            last_mtime_sec: 0,
            failure: Default::default(),
            termination: Default::default(),
        }
    }
}

impl FromStr for Service {
    type Err = HorustError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        toml::from_str::<Service>(s).map_err(HorustError::from)
    }
}

/// A user in the system.
/// It can be either a uuid or a username (available in passwd)
#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(untagged)]
pub enum User {
    Uid(u32),
    Name(String),
}

impl From<unistd::Uid> for User {
    fn from(uid: unistd::Uid) -> Self {
        User::Uid(uid.as_raw())
    }
}

impl Default for User {
    fn default() -> Self {
        unistd::getuid().into()
    }
}
impl User {
    pub(crate) fn get_uid(&self) -> unistd::Uid {
        match &self {
            //TODO: getpwuid_r is not available in unistd.
            User::Name(name) => unistd::User::from_name(name).unwrap().unwrap().uid,
            User::Uid(uid) => unistd::Uid::from_raw(uid.clone()),
        }
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
impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.write_str(match self {
            ServiceStatus::Starting => "Starting",
            ServiceStatus::ToBeRun => "ToBeRun",
            ServiceStatus::Running => "Running",
            ServiceStatus::InKilling => "InKilling",
            ServiceStatus::Finished => "Finished",
            ServiceStatus::Failed => "Failed",
            ServiceStatus::Initial => "Initial",
        })
    }
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

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Failure {
    #[serde(default = "Failure::default_exit_code")]
    pub exit_code: Vec<i32>,
    pub strategy: FailureStrategy,
}
impl Failure {
    fn default_exit_code() -> Vec<i32> {
        (1..).take(255).collect()
    }
}
#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum FailureStrategy {
    KillAll,
    KillDepdency,
    Ignore,
}
impl Default for Failure {
    fn default() -> Self {
        Failure {
            exit_code: Self::default_exit_code(),
            strategy: FailureStrategy::Ignore,
        }
    }
}

impl From<String> for FailureStrategy {
    fn from(strategy: String) -> Self {
        strategy.as_str().into()
    }
}

impl From<&str> for FailureStrategy {
    fn from(strategy: &str) -> Self {
        match strategy.to_lowercase().as_str() {
            "kill-depdency" => FailureStrategy::KillDepdency,
            "kill-all" => FailureStrategy::KillAll,
            "ignore" => FailureStrategy::Ignore,
            _ => FailureStrategy::Ignore,
        }
    }
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Termination {
    #[serde(default)]
    pub(crate) signal: TerminationSignal,
    #[serde(default = "Termination::default_wait", with = "humantime_serde")]
    pub wait: Duration,
}

impl Termination {
    fn default_wait() -> Duration {
        Duration::from_secs(5)
    }
}

impl Default for Termination {
    fn default() -> Self {
        Termination {
            signal: Default::default(),
            wait: Self::default_wait(),
        }
    }
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
pub enum TerminationSignal {
    TERM,
    HUP,
    INT,
    QUIT,
    KILL,
    USR1,
    USR2,
}
impl TerminationSignal {
    pub(crate) fn as_signal(&self) -> Signal {
        match self {
            TerminationSignal::TERM => SIGTERM,
            TerminationSignal::HUP => SIGHUP,
            TerminationSignal::INT => SIGINT,
            TerminationSignal::QUIT => SIGQUIT,
            TerminationSignal::KILL => SIGKILL,
            TerminationSignal::USR1 => SIGUSR1,
            TerminationSignal::USR2 => SIGUSR2,
        }
    }
}
impl Default for TerminationSignal {
    fn default() -> Self {
        TerminationSignal::TERM
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
                user: Default::default(),
                restart: Default::default(),
                start_delay: Duration::from_secs(0),
                command: "".to_string(),
                healthiness: None,
                signal_rewrite: None,
                last_mtime_sec: 0,
                failure: Default::default(),
                termination: Default::default(),
            }
        }

        pub fn from_name(name: &str) -> Self {
            Self::start_after(name, Vec::new())
        }
    }
    #[test]
    fn test_should_correctly_deserialize_sample() {
        let service = Service::from_str(get_sample_service().as_str());
        service.expect("error on deserializing the manifest");
    }
}
