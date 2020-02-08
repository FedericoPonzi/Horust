use crate::horust::HorustError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

pub type ServiceName = String;

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Service {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub working_directory: PathBuf,
    #[serde(default, with = "humantime_serde")]
    pub start_delay: Duration,
    #[serde(default = "Vec::new")]
    pub start_after: Vec<ServiceName>,
    #[serde(default)]
    pub restart_strategy: RestartStrategy,
    #[serde(default, with = "humantime_serde")]
    pub restart_backoff: Duration,
    #[serde()]
    pub healthness: Option<Healthness>,
}

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Healthness {
    pub http_endpoint: Option<String>,
    pub file_path: Option<PathBuf>,
}

impl Service {
    pub fn get_sample_service() -> String {
        r#"name = "my-cool-service"
command = ""
working-directory = "/tmp/"
restart = "never"
start-delay = "2s"
#restart-backoff = "10s"
[healthness]
http_endpoint = "http://localhost:8080/healthcheck"
file_path = "/var/myservice/up""#
            .to_string()
    }
    pub fn from_file(path: PathBuf) -> Result<Self, HorustError> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str::<Service>(content.as_str()).map_err(HorustError::from)
    }
}
impl FromStr for Service {
    type Err = HorustError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        toml::from_str::<Service>(s).map_err(HorustError::from)
    }
}

/// Visualize: https://state-machine-cat.js.org/
/// initial => Initial : "Will eventually be run";
//Initial => ToBeRun : "All dependencies are running, a thread has spawned and will run the fork/exec the process";
//ToBeRun => Starting : "The ServiceHandler has a pid";
//Starting => Running : "The service has met healthness policy";
//Starting => Failed : "Service cannot be started";
//Running => Finished : "Exit status = 0";
//Running => Failed  : "Exit status != 0";
//Finished => Initial : "restart = Always";
//Failed => Initial : "restart = always|on-failure";
#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
pub enum ServiceStatus {
    Starting,
    /// This is just an intermediate state between Initial and Running.
    ToBeRun,
    /// The service is up and healthy
    Running,
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
pub enum RestartStrategy {
    Always,
    OnFailure,
    Never,
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

impl Default for RestartStrategy {
    fn default() -> Self {
        RestartStrategy::Never
    }
}

#[cfg(test)]
mod test {
    use crate::horust::formats::{RestartStrategy, Service};
    use std::time::Duration;

    impl Service {
        pub fn start_after(name: &str, start_after: Vec<&str>) -> Self {
            Service {
                name: name.to_owned(),
                start_after: start_after.into_iter().map(|v| v.into()).collect(),
                working_directory: "".into(),
                restart_strategy: RestartStrategy::Always,
                start_delay: Duration::from_secs(0),
                command: "".to_string(),
                restart_backoff: Default::default(),
                healthness: None,
            }
        }
        pub fn from_name(name: &str) -> Self {
            Self::start_after(name, Vec::new())
        }
    }
    #[test]
    fn test_should_deserialize_sample() {
        let des = toml::from_str::<Service>(Service::get_sample_service().as_ref());
        assert!(des.is_ok())
    }
    // TODO: usa sample to verify the correctness of deserialization.
    #[test]
    #[ignore]
    fn test_should_correctly_deserialize_sample() {}
    #[test]
    pub fn test_should_correctly_deserialize() {
        let name = "my-cool-service";
        let command = "/home/isaacisback/dev/rust/horust/examples/services/first.sh";
        let working_directory = "/tmp/";
        let restart = "always";
        let start_delay = "1s";
        let restart_backoff = "10s";
        let service = format!(
            r#"
name = "{}"
command = "{}"
working-directory = "{}"
restart-strategy = "{}"
restart-backoff = "{}"
start-delay = "{}"
"#,
            name, command, working_directory, restart, restart_backoff, start_delay
        );
        let des = toml::from_str(&service);
        let des: Service = des.unwrap();
        let expected = Service {
            name: name.into(),
            command: command.into(),
            working_directory: working_directory.into(),
            start_delay: Duration::from_secs(1),
            start_after: vec![],
            restart_strategy: restart.into(),
            restart_backoff: Duration::from_secs(10),
            healthness: None,
        };
        assert_eq!(des, expected);
    }
}
