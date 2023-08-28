use nix::unistd::Pid;

pub use service::*;

mod service;

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum ShuttingDown {
    Gracefully,
    Forcefully,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    PidChanged(ServiceName, Pid),
    // This command updates the service status.
    StatusUpdate(ServiceName, ServiceStatus),
    // This event represents a status change.
    StatusChanged(ServiceName, ServiceStatus),
    ServiceExited(ServiceName, i32),
    ForceKill(ServiceName),
    Kill(ServiceName),
    SpawnFailed(ServiceName),
    Run(ServiceName),
    ShuttingDownInitiated(ShuttingDown),
    HealthCheck(ServiceName, HealthinessStatus),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitStatus {
    Successful,
    SomeServiceFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
