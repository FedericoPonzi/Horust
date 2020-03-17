use crate::horust::error::{ValidationError, ValidationErrorKind};
use crate::horust::HorustError;
use nix::sys::signal::Signal;
use nix::sys::signal::{SIGHUP, SIGINT, SIGKILL, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2};
use nix::unistd;
use serde::export::fmt::Error;
use serde::export::Formatter;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

pub fn get_sample_service() -> String {
    r#"# The name of your service, must be unique. It's optional, will use the filename as name.
name = "my-cool-service"
command = "/bin/bash -c 'echo hello world'"
working-directory = "/tmp/"
start-delay = "2s"
start-after = ["another.toml", "second.toml"]
user = "root"

[restart]
strategy = "never"
backoff = "0s"
attempts = 0

[healthiness]
http_endpoint = "http://localhost:8080/healthcheck"
file_path = "/var/myservice/up"

[failure]
successfull_exit_code = [ 0, 1, 255]
strategy = "ignore"

[termination]
signal = "TERM"
wait = "10s"
"#
    .to_string()
}

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
    pub environment: Option<Environment>,
    pub working_directory: Option<PathBuf>,
    #[serde(default, with = "humantime_serde")]
    pub start_delay: Duration,
    #[serde(default = "Vec::new")]
    pub start_after: Vec<ServiceName>,
    #[serde(default)]
    pub restart: Restart,
    pub healthiness: Option<Healthness>,
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
pub struct Environment {
    #[serde(flatten)]
    pub key_val: HashMap<String, String>,
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Healthness {
    pub http_endpoint: Option<String>,
    pub file_path: Option<PathBuf>,
}

impl Service {
    pub fn from_file(path: &PathBuf) -> Result<Self, HorustError> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str::<Service>(content.as_str()).map_err(HorustError::from)
    }

    /// Create the environment K=V variables, used for exec into the new process.
    /// User defined environment variables overwrite the predefined variables.
    pub fn get_environment(&self) -> Vec<String> {
        let mut additional = self
            .environment
            .clone()
            .map(|env| env.key_val)
            .unwrap_or_else(HashMap::new);
        let get_env = |name: &str, default: &str| {
            (
                name.to_string(),
                std::env::var(name).unwrap_or_else(|_| default.to_string()),
            )
        };
        let hostname = get_env("HOSTNAME", "localhost");
        let path = get_env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        );
        let user = ("USER".to_string(), self.user.get_name());
        let home = (
            "HOME".to_string(),
            self.user.get_home().display().to_string(),
        );
        let env: HashMap<String, String> = vec![hostname, path, user, home].into_iter().collect();
        env.into_iter().for_each(|(k, v)| {
            additional.entry(k).or_insert(v);
        });

        // Since I don't know a sane default:
        if let Ok(term) = std::env::var("TERM") {
            additional.insert("TERM".into(), term);
        }

        additional
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect()
    }

    pub fn from_command(command: String) -> Self {
        Service {
            name: command.clone(),
            start_after: Default::default(),
            user: Default::default(),
            environment: None,
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
            User::Name(name) => unistd::User::from_name(name).unwrap().unwrap().uid,
            User::Uid(uid) => unistd::Uid::from_raw(*uid),
        }
    }

    fn get_raw_user(&self) -> unistd::User {
        unistd::User::from_uid(self.get_uid()).unwrap().unwrap()
    }

    fn get_home(&self) -> PathBuf {
        self.get_raw_user().dir
    }

    fn get_name(&self) -> String {
        self.get_raw_user().name
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
    /// Friendly signal sent, waiting for the process to terminate.
    InKilling,
    InRunning,
    /// A finished service has done it's job and won't be restarted.
    Finished,
    /// A Failed service might be restarted if the restart policy demands so.
    Failed,
    // A Service that will be killed soon.
    ToBeKilled,
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
            ServiceStatus::ToBeKilled => "ToBeKilled",
            ServiceStatus::Initial => "Initial",
            ServiceStatus::InRunning => "InRunning",
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
    #[serde(default = "Failure::default_successfull_exit_code")]
    pub successfull_exit_code: Vec<i32>,
    pub strategy: FailureStrategy,
}

impl Failure {
    fn default_successfull_exit_code() -> Vec<i32> {
        vec![0]
    }
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum FailureStrategy {
    Shutdown,
    KillDependents,
    Ignore,
}

impl Default for Failure {
    fn default() -> Self {
        Failure {
            successfull_exit_code: Self::default_successfull_exit_code(),
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
            "kill-dependents" => FailureStrategy::KillDependents,
            "kill-all" => FailureStrategy::Shutdown,
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

/// Runs some validation checks on the services.
pub fn validate(services: Vec<Service>) -> Result<Vec<Service>, Vec<ValidationError>> {
    let mut errors = vec![];
    services.iter().for_each(|service| {
        if !service.start_after.is_empty() {
            debug!(
                "Checking if all depedencies of '{}' exists, deps: {:?}",
                service.name, service.start_after
            );
        }
        service
            .start_after
            .iter()
            .for_each(|name| {
                let passed = services.iter().any(|s| s.name == *name);
                if !passed {
                    let err = format!("Service '{}', should start after '{}', but there is no service with such name.", service.name, name);
                    errors.push(ValidationError::new(err.as_str(), ValidationErrorKind::MissingDependency));
                }
            });
    });
    if errors.is_empty() {
        Ok(services)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod test {
    use crate::horust::formats::{validate, Service};
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
                environment: None,
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
    #[test]
    fn test_validate() {
        // Service does not exists:
        let services = vec![Service::start_after("a", vec!["b"])];
        validate(services).unwrap_err();

        // Should pass validation:
        let services = vec![
            Service::from_name("b"),
            Service::start_after("a", vec!["b"]),
        ];
        validate(services).expect("Validation failed");
    }
}
