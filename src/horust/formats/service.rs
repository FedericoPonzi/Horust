use crate::horust::error::{HorustError, ValidationError, ValidationErrorKind};
use nix::sys::signal::{Signal, SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2};
use nix::unistd;
use serde::de::{self, Visitor};
use serde::export::fmt::Error;
use serde::export::Formatter;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use templar::*;

pub fn get_sample_service() -> String {
    r#"command = "/bin/bash -c 'echo hello world'"
start-delay = "2s"
start-after = ["another.toml", "second.toml"]
stdout = "STDOUT"
stderr = "/var/logs/hello_world_svc/stderr.log"
user = "{{ env('USER') }}"
working-directory = "/tmp/"

[restart]
strategy = "never"
backoff = "0s"
attempts = 0

[healthiness]
http-endpoint = "http://localhost:8080/healthcheck"
file-path = "/var/myservice/up"
# Max healthchecks allowed to fail in a row before considering this service failed.
max-failed = 3

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
    pub name: ServiceName,
    #[serde()]
    pub command: String,
    #[serde(default)]
    pub user: User,
    #[serde(default = "Service::default_working_directory")]
    pub working_directory: PathBuf,
    #[serde(default = "Service::default_stdout_log")]
    pub stdout: LogOutput,
    #[serde(default = "Service::default_stderr_log")]
    pub stderr: LogOutput,
    #[serde(default, with = "humantime_serde")]
    pub start_delay: Duration,
    #[serde(default = "Vec::new")]
    pub start_after: Vec<ServiceName>,
    #[serde()]
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

    fn default_stdout_log() -> LogOutput {
        LogOutput::Stdout
    }

    fn default_stderr_log() -> LogOutput {
        LogOutput::Stderr
    }

    /// Tries to load specific config from path.
    /// Config will be automatically templated from env.
    /// Correct syntax is required for templating to work.
    /// Check documentation on templating for more info.
    /// Currently only templating from environment is implemented.
    pub fn from_file(path: &PathBuf) -> crate::horust::error::Result<Self> {
        let preconfig = std::fs::read_to_string(path)?;
        let template = Templar::global().parse(&preconfig)?;
        let context = StandardContext::new();
        let postconfig = template.render(&context)?;
        toml::from_str::<Service>(&postconfig).map_err(HorustError::from)
    }
    /// Creates the environment K=V variables, used for exec into the new process.
    /// User defined environment variables overwrite the predefined values.
    pub fn get_environment(&self) -> crate::horust::error::Result<Vec<String>> {
        Ok(self.environment.get_environment(
            self.user.clone().get_name()?,
            self.user.clone().get_home()?.display().to_string(),
        ))
    }

    /// Wrapper for single command horust run
    pub fn from_command(command: String) -> Self {
        Service {
            name: command.clone(),
            command,
            ..Default::default()
        }
    }
}
impl Default for Service {
    fn default() -> Self {
        Self {
            name: "".to_owned(),
            start_after: Default::default(),
            working_directory: "/".into(),
            stdout: Default::default(),
            stderr: Default::default(),
            user: Default::default(),
            restart: Default::default(),
            start_delay: Duration::from_secs(0),
            command: "command".to_string(),
            healthiness: Default::default(),
            signal_rewrite: None,
            environment: Default::default(),
            failure: Default::default(),
            termination: Default::default(),
        }
    }
}

impl FromStr for Service {
    type Err = HorustError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let template = Templar::global().parse(s)?;
        let context = StandardContext::new();

        let postconfig = template.render(&context)?;
        toml::from_str::<Service>(&postconfig).map_err(HorustError::from)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LogOutput {
    Stderr,
    Stdout,
    Path(PathBuf),
}

impl Serialize for LogOutput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let as_string: String = self.clone().into();
        serializer.serialize_str(as_string.as_str())
    }
}

impl<'de> Deserialize<'de> for LogOutput {
    fn deserialize<D>(deserializer: D) -> Result<LogOutput, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(LogOutputVisitor)
    }
}

struct LogOutputVisitor;
impl<'de> Visitor<'de> for LogOutputVisitor {
    type Value = LogOutput;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string with 'STDOUT', 'STDERR', or a full path. All as `String`s ")
    }
    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(LogOutput::from(value))
    }
}

impl Default for LogOutput {
    fn default() -> Self {
        Self::Stdout
    }
}

impl From<String> for LogOutput {
    fn from(strategy: String) -> Self {
        strategy.as_str().into()
    }
}
impl Into<String> for LogOutput {
    fn into(self) -> String {
        match self {
            Self::Stdout => "STDOUT".to_string(),
            Self::Stderr => "STDERR".to_string(),
            Self::Path(path) => {
                let path = path.display();
                path.to_string()
            }
        }
    }
}

impl From<&str> for LogOutput {
    fn from(strategy: &str) -> Self {
        match strategy {
            "STDOUT" => LogOutput::Stdout,
            "STDERR" => LogOutput::Stderr,
            path => LogOutput::Path(PathBuf::from(path)),
        }
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
pub struct Healthiness {
    pub http_endpoint: Option<String>,
    pub file_path: Option<PathBuf>,
    #[serde(default = "Healthiness::default_max_failed")]
    pub max_failed: i32,
}
impl Healthiness {
    fn default_max_failed() -> i32 {
        3
    }
}
impl Default for Healthiness {
    fn default() -> Self {
        Self {
            http_endpoint: None,
            file_path: None,
            max_failed: 3,
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
    pub(crate) fn get_uid(&self) -> crate::horust::error::Result<unistd::Uid> {
        match &self {
            User::Name(name) => unistd::User::from_name(name)
                .map_err(HorustError::from)
                .and_then(|opt| {
                    opt.ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::NotFound, "User not found")
                    })
                    .map_err(HorustError::from)
                    .map(|user| user.uid)
                }),
            User::Uid(uid) => Ok(unistd::Uid::from_raw(*uid)),
        }
    }

    fn get_raw_user(&self) -> crate::horust::error::Result<unistd::User> {
        unistd::User::from_uid(self.get_uid()?)
            .map_err(HorustError::from)
            .and_then(|opt| {
                opt.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "User not found")
                })
                .map_err(HorustError::from)
            })
    }

    fn get_home(&self) -> crate::horust::error::Result<PathBuf> {
        Ok(self.get_raw_user()?.dir)
    }

    fn get_name(&self) -> crate::horust::error::Result<String> {
        Ok(self.get_raw_user()?.name)
    }
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq, Hash)]
pub enum ServiceStatus {
    /// The service will be started asap
    Starting,
    /// Service has a pid
    Started,
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
    /// This is the initial state: A service in Initial state is marked to be runnable:
    /// it will be run as soon as possible.
    Initial,
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.write_str(match self {
            ServiceStatus::Failed => "Failed",
            ServiceStatus::Finished => "Finished",
            ServiceStatus::FinishedFailed => "FinishedFailed",
            ServiceStatus::InKilling => "InKilling",
            ServiceStatus::Initial => "Initial",
            ServiceStatus::Running => "Running",
            ServiceStatus::Started => "Started",
            ServiceStatus::Starting => "Starting",
            ServiceStatus::Success => "Success",
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
    0
}

impl Default for Restart {
    fn default() -> Self {
        Restart {
            strategy: Default::default(),
            backoff: Duration::from_secs(0),
            attempts: default_attempts(),
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

#[derive(Serialize, Copy, Clone, Deserialize, Debug, Eq, PartialEq)]
pub enum TerminationSignal {
    TERM,
    HUP,
    INT,
    QUIT,
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
/// TODO: if redirect output is file, check it exists and permissions.
pub fn validate(services: Vec<Service>) -> Result<Vec<Service>, Vec<ValidationError>> {
    let mut errors = vec![];
    services.iter().for_each(|service| {
        if service.command.is_empty() {
            let err = format!("Command is defined, but it is empty for service: {}", service.name);
            errors.push(ValidationError::new(err.as_str(), ValidationErrorKind::CommandEmpty));
        }
        if !service.start_after.is_empty() {
            debug!(
                "Checking if all depedencies of '{}' exists, deps: {:?}",
                service.name, service.start_after
            );
        }
        service.start_after.iter().for_each(|name| {
            let passed = services.iter().any(|s| s.name == *name);
            if !passed {
                let err = format!(
                    "Service '{}', should start after '{}', but there is no service with such name.",
                    service.name, name
                );
                errors.push(ValidationError::new(
                    err.as_str(),
                    ValidationErrorKind::MissingDependency,
                ));
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
    use crate::horust::formats::{
        validate, Environment, Failure, FailureStrategy, Healthiness, Restart, RestartStrategy,
        Service, Termination, TerminationSignal::TERM,
    };
    use crate::horust::get_sample_service;
    use std::str::FromStr;
    use std::time::Duration;

    impl Service {
        pub fn start_after(name: &str, start_after: Vec<&str>) -> Self {
            Self {
                name: name.to_owned(),
                start_after: start_after.into_iter().map(|v| v.into()).collect(),
                ..Default::default()
            }
        }

        pub fn from_name(name: &str) -> Self {
            Self::start_after(name, Vec::new())
        }
    }

    #[test]
    fn test_should_correctly_deserialize_sample() {
        let current_user_name: String = super::User::default().get_name().unwrap();
        let expected = Service {
            name: "".to_string(),
            command: "/bin/bash -c \'echo hello world\'".to_string(),
            user: super::User::Name(current_user_name),
            environment: Environment {
                keep_env: false,
                re_export: vec!["PATH".to_string(), "DB_PASS".to_string()],
                additional: vec![("key".to_string(), "value".to_string())]
                    .into_iter()
                    .collect(),
            },
            working_directory: "/tmp/".into(),
            stdout: "STDOUT".into(),
            stderr: "/var/logs/hello_world_svc/stderr.log".into(),
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
                ..Default::default()
            },
            signal_rewrite: None,
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

        // Command is empty:
        let services = vec![Service::from_command("".into())];
        validate(services).unwrap_err();

        // Should pass validation:
        let services = vec![
            Service::from_name("b"),
            Service::start_after("a", vec!["b"]),
        ];
        validate(services).expect("Validation failed");
    }
}
