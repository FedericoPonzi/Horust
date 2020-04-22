mod horust_config;
mod service;
pub use horust_config::HorustConfig;
use nix::unistd::Pid;
pub use service::*;

#[derive(Debug, Clone, PartialEq)]
pub enum ExitStatus {
    Successful,
    SomeServiceFailed,
}

pub type ComponentName = String;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    PidChanged(ServiceName, Pid),
    StatusChanged(ServiceName, ServiceStatus),
    ServiceExited(ServiceName, i32),
    ForceKill(ServiceName),
    Kill(ServiceName),
    Run(ServiceName),
    Exiting(ComponentName, ExitStatus),
    ShuttingDownInitiated,
    // TODO: to allow changes of service at runtime:
    //ServiceCreated(ServiceHandler),
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
    pub(crate) fn new_exit_success(comp_name: &str) -> Self {
        Event::Exiting(comp_name.into(), ExitStatus::Successful)
    }
}
