mod horust_config;
mod service;
pub use horust_config::HorustConfig;
use nix::unistd::Pid;
pub use service::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    PidChanged(ServiceName, Pid),
    ServiceStarted(ServiceName),
    // This command updates the service status.
    StatusUpdate(ServiceName, ServiceStatus),
    // This event represents a status change.
    StatusChanged(ServiceName, ServiceStatus),
    ServiceExited(ServiceName, i32),
    ForceKill(ServiceName),
    Kill(ServiceName),
    SpawnFailed(ServiceName),
    Run(ServiceName),
    ShuttingDownInitiated,
    HealthCheck(ServiceName, HealthinessStatus),
    // TODO: to allow changes of service at supervisor:
    //ServiceCreated(ServiceHandler)
}

impl Event {
    pub(crate) fn new_pid_changed(service_name: ServiceName, pid: Pid) -> Self {
        Self::PidChanged(service_name, pid)
    }
    pub(crate) fn new_status_changed(service_name: &str, status: ServiceStatus) -> Self {
        Self::StatusChanged(service_name.to_string(), status)
    }
    pub(crate) fn new_status_update(service_name: &str, status: ServiceStatus) -> Self {
        Self::StatusUpdate(service_name.to_string(), status)
    }
    pub(crate) fn new_service_exited(service_name: ServiceName, exit_status: i32) -> Self {
        Self::ServiceExited(service_name, exit_status)
    }
    pub(crate) fn new_force_kill(service_name: &str) -> Self {
        Self::ForceKill(service_name.to_string())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExitStatus {
    Successful,
    SomeServiceFailed,
}

#[derive(PartialEq, Clone, Debug)]
pub enum HealthinessStatus {
    Healthy,
    Unhealthy,
}

impl From<bool> for HealthinessStatus {
    fn from(check: bool) -> Self {
        if check {
            HealthinessStatus::Healthy
        } else {
            HealthinessStatus::Unhealthy
        }
    }
}
