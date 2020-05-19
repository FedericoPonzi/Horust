mod horust_config;
mod service;
pub use horust_config::HorustConfig;
use nix::unistd::Pid;
pub use service::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Component {
    Reaper,
    Runtime,
    Healthchecker
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    PidChanged(ServiceName, Pid),
    ServiceStarted(ServiceName),
    StatusChanged(ServiceName, ServiceStatus),
    ServiceExited(ServiceName, i32),
    ForceKill(ServiceName),
    Kill(ServiceName),
    SpawnFailed(ServiceName),
    Run(ServiceName),
    Exiting(Component, ExitStatus),
    ShuttingDownInitiated,
    HealthCheck(ServiceName, HealthinessStatus),
    // TODO: to allow changes of service at runtime:
    //ServiceCreated(ServiceHandler)
}

impl Event {
    pub(crate) fn new_pid_changed(service_name: ServiceName, pid: Pid) -> Self {
        Self::PidChanged(service_name, pid)
    }
    pub(crate) fn new_status_changed(service_name: &ServiceName, status: ServiceStatus) -> Self {
        Self::StatusChanged(service_name.clone(), status)
    }
    pub(crate) fn new_service_exited(service_name: ServiceName, exit_status: i32) -> Self {
        Self::ServiceExited(service_name, exit_status)
    }
    pub(crate) fn new_force_kill(service_name: &ServiceName) -> Self {
        Self::ForceKill(service_name.clone())
    }
    pub(crate) fn new_exit_success(component: Component) -> Self {
        Event::Exiting(component, ExitStatus::Successful)
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
