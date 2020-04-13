use crate::horust::error::{HorustError, ValidationError, ValidationErrorKind};
use nix::sys::signal::{Signal, SIGHUP, SIGINT, SIGKILL, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2};
use nix::unistd;
use serde::export::fmt::Error;
use serde::export::Formatter;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

pub fn get_sample_service() -> String {
    r#"
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
http-endpoint = "http://localhost:8080/healthcheck"
file-path = "/var/myservice/up"

[failure]
successful-exit-code = [ 0, 1, 255]
strategy = "ignore"

[environment]
keep-env = false
re-export = [ "PATH", "DB_PASS"]
additional = { key = "value"} 

[termination]
signal = "TERM"
wait = "10s"
die-if-failed  = [ "db.toml"]
"#
    .to_string()
}

pub type ServiceName = String;

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Service {
    #[serde(default)]
    //todo: length should be > 0.
    pub name: ServiceName,
    #[serde()]
    //todo: length should be > 0.
    pub command: String,
    #[serde(default)]
    pub user: User,
    #[serde(default = "Service::default_working_directory")]
    pub working_directory: PathBuf,
    #[serde(default, with = "humantime_serde")]
    pub start_delay: Duration,
    #[serde(default = "Vec::new")]
    pub start_after: Vec<ServiceName>,
    #[serde(skip)]
    pub last_mtime_sec: i64,
    pub signal_rewrite: Option<String>,
    #[serde(default)]
    pub restart: Restart,
    #[serde(default)]
    pub healthiness: Healthiness,
    #[serde(default)]
    pub failure: Failure,
    #[serde(default)]
    pub environment: Environment,
    #[serde(default)]
    pub termination: Termination,
}
impl Service {
    fn default_working_directory() -> PathBuf {
        PathBuf::from("/")
    }
    pub fn from_file(path: &PathBuf) -> crate::horust::error::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str::<Service>(content.as_str()).map_err(HorustError::from)
    }

    /// Create the environment K=V variables, used for exec into the new process.
    /// User defined environment variables overwrite the predefined variables.
    pub fn get_environment(&self) -> Vec<String> {
        self.environment.get_environment(
            self.user.get_name().clone(),
            self.user.get_home().display().to_string(),
        )
    }

    pub fn from_command(command: String) -> Self {
        Service {
            name: command.clone(),
            start_after: Default::default(),
            user: Default::default(),
            environment: Default::default(),
            working_directory: "/".into(),
            restart: Default::default(),
            start_delay: Duration::from_secs(0),
            command,
            healthiness: Default::default(),
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

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Environment {
    #[serde(default = "Environment::default_keep_env")]
    pub keep_env: bool,
    #[serde(default)]
    pub re_export: Vec<String>,
    #[serde(default)]
    pub additional: HashMap<String, String>,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            keep_env: false,
            re_export: Default::default(),
            additional: Default::default(),
        }
    }
}

impl Environment {
    fn default_keep_env() -> bool {
        true
    }

    fn get_hostname_val() -> String {
        let hostname_path = "/etc/hostname";
        let localhost = "localhost".to_string();
        if std::path::PathBuf::from(hostname_path).is_file() {
            std::fs::read_to_string(hostname_path).unwrap_or_else(|_| localhost)
        } else {
            std::env::var("HOSTNAME").unwrap_or_else(|_| localhost)
        }
    }

    /// Create the environment K=V variables, used for exec into the new process.
    /// User defined environment variables overwrite the predefined variables.
    pub(crate) fn get_environment(&self, user_name: String, user_home: String) -> Vec<String> {
        let mut initial = if self.keep_env {
            std::env::vars().collect()
        } else {
            HashMap::new()
        };

        let mut additional = self.additional.clone();

        let get_env = |name: &str, default: &str| {
            (
                name.to_string(),
                std::env::var(name).unwrap_or_else(|_| default.to_string()),
            )
        };
        let hostname = ("HOSTNAME".to_string(), Self::get_hostname_val());
        let path_env = get_env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/games:/usr/local/games",
        );
        let user_name = ("USER".to_string(), user_name);
        let user_home = ("HOME".to_string(), user_home);

        let env: HashMap<String, String> = vec![hostname, path_env, user_name, user_home]
            .into_iter()
            .collect();
        // The variables from env have always precedence over initial. E.g. home, and user might differ.
        initial.extend(env);

        // Since I don't know a sane default:
        if let Ok(term) = std::env::var("TERM") {
            initial.entry("TERM".to_string()).or_insert(term);
        }

        let re_export: HashMap<String, String> = self
            .re_export
            .iter()
            .filter_map(|key| {
                std::env::var(key)
                    .map_err(|err| error!("Error getting env key: {}, error: {} ", key, err))
                    .ok()
                    .map(|value| (key.clone(), value))
            })
            .collect();

        // If a variable is re_export, then it has precedence over initial + env.
        initial.extend(re_export);

        // Finally, additional has the higher precedence:
        initial.into_iter().for_each(|(k, v)| {
            additional.entry(k).or_insert(v);
        });

        // This is the suitable format for `exec`
        additional
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect()
    }
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
// TODO: Add a retry instead of instantly giving up.
pub struct Healthiness {
    pub http_endpoint: Option<String>,
    pub file_path: Option<PathBuf>,
}

impl Default for Healthiness {
    fn default() -> Self {
        Self {
            http_endpoint: None,
            file_path: None,
        }
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
            User::Name(name) => {
                unistd::User::from_name(name)
                    .expect("Failed getting the user")
                    .expect("User does not exists")
                    .uid
            }
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
Failed => FinishedFailed : "Restart policy ";
Running => ToBeKilled: "Marked for killing";
ToBeKilled => InKilling : "Friendly TERM signal sent";
InKilling => Finished : "Successfully killed";
InKilling => FinishedFailed : "Forcefully killed (SIGKILL)";
Running => Failed  : "Exit status is not successful";
Running => Success  : "Exit status == 0";
Success => Initial : "Restart policy applied";
Success => Finished : "Based on restart policy";
Failed => Initial : "restart = always|on-failure";
*/

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
pub enum ServiceStatus {
    /// Has a pid,
    Starting,
    /// This is just an intermediate state between Initial and Running.
    ToBeRun,
    /// The service is up and healthy
    Running,
    /// Friendly signal sent, waiting for the process to terminate.
    InKilling,
    /// A successfully exited service.
    Success,
    /// A finished service has done it's job and won't be restarted.
    Finished,
    /// A failed, finished service won't be restarted.
    FinishedFailed,
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
            ServiceStatus::Success => "Success",
            ServiceStatus::FinishedFailed => "FinishedFailed",
        })
    }
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Restart {
    #[serde(default)]
    pub strategy: RestartStrategy,
    #[serde(default, with = "humantime_serde")]
    pub backoff: Duration,
    #[serde(default = "default_attempts")]
    pub attempts: u32,
}
fn default_attempts() -> u32 {
    10
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
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
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
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Failure {
    #[serde(default = "Failure::default_successful_exit_code")]
    pub successful_exit_code: Vec<i32>,
    pub strategy: FailureStrategy,
}

impl Failure {
    fn default_successful_exit_code() -> Vec<i32> {
        vec![0]
    }
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum FailureStrategy {
    Shutdown,
    KillDependents,
    Ignore,
}

impl Default for Failure {
    fn default() -> Self {
        Failure {
            successful_exit_code: Self::default_successful_exit_code(),
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
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Termination {
    #[serde(default)]
    /// Use this signal instead of SIGTERM.
    pub(crate) signal: TerminationSignal,
    #[serde(default = "Termination::default_wait", with = "humantime_serde")]
    /// Time to wait before SIGKILL
    pub wait: Duration,
    #[serde(default = "Vec::new")]
    // Will kill this service if any of the services in Vec are failed
    pub die_if_failed: Vec<ServiceName>,
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
            die_if_failed: Vec::new(),
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
impl Into<Signal> for TerminationSignal {
    fn into(self) -> Signal {
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
    use crate::horust::formats::TerminationSignal::TERM;
    use crate::horust::formats::User::Name;
    use crate::horust::formats::{
        validate, Environment, Failure, FailureStrategy, Healthiness, Restart, RestartStrategy,
        Service, Termination,
    };
    use crate::horust::get_sample_service;
    use std::str::FromStr;
    use std::time::Duration;

    impl Service {
        pub fn start_after(name: &str, start_after: Vec<&str>) -> Self {
            Service {
                name: name.to_owned(),
                start_after: start_after.into_iter().map(|v| v.into()).collect(),
                working_directory: "".into(),
                user: Default::default(),
                restart: Default::default(),
                start_delay: Duration::from_secs(0),
                command: "".to_string(),
                healthiness: Default::default(),
                signal_rewrite: None,
                environment: Default::default(),
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
        let expected = Service {
            name: "".to_string(),
            command: "/bin/bash -c \'echo hello world\'".to_string(),
            user: Name("root".into()),
            environment: Environment {
                keep_env: false,
                re_export: vec!["PATH".to_string(), "DB_PASS".to_string()],
                additional: vec![("key".to_string(), "value".to_string())]
                    .into_iter()
                    .collect(),
            },
            working_directory: "/tmp/".into(),
            start_delay: Duration::from_secs(2),
            start_after: vec!["another.toml".into(), "second.toml".into()],
            restart: Restart {
                strategy: RestartStrategy::Never,
                backoff: Duration::from_millis(0),
                attempts: 0,
            },
            healthiness: Healthiness {
                http_endpoint: Some("http://localhost:8080/healthcheck".into()),
                file_path: Some("/var/myservice/up".into()),
            },
            signal_rewrite: None,
            last_mtime_sec: 0,
            failure: Failure {
                successful_exit_code: vec![0, 1, 255],
                strategy: FailureStrategy::Ignore,
            },
            termination: Termination {
                signal: TERM,
                wait: Duration::from_secs(10),
                die_if_failed: vec!["db.toml".into()],
            },
        };
        let service = Service::from_str(get_sample_service().as_str())
            .expect("error on deserializing the manifest");
        assert_eq!(expected, service);
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
