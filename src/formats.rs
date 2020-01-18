use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

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

#[derive(Serialize, Clone, Deserialize, Debug, Eq, PartialEq)]
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
    #[serde(default, with = "humantime_serde")]
    pub restart_backoff: Duration,
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
    #[test]
    pub fn test_should_deserialize() {
        let name = "my-cool-service";
        let command = "/home/isaacisback/dev/rust/horust/examples/services/first.sh";
        let working_directory = "/tmp/";
        let restart = "always";
        let restart_backoff = "10s";
        let service = format!(
            r#"
name = "{}"
command = "{}"
working-directory = "{}"
restart = "{}"
restart-backoff = "{}"
"#,
            name, command, working_directory, restart, restart_backoff
        );
        let des = toml::from_str(&service);
        let des: Service = des.unwrap();
        let expected = Service {
            name: name.into(),
            command: command.into(),
            working_directory: working_directory.into(),
            start_delay: Default::default(),
            start_after: vec![],
            restart: restart.into(),
            restart_backoff: Duration::from_secs(10),
        };
        assert_eq!(des, expected);
    }
}
