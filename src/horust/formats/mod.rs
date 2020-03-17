mod service;
mod service_handler;
use nix::unistd::Pid;
pub use service::*;
pub use service_handler::ServiceHandler;

#[derive(Debug, Clone)]
pub enum Event {
    PidChanged(ServiceName, Pid),
    StatusChanged(ServiceName, ServiceStatus),
    ServiceExited(ServiceName, i32),
    ForceKill(ServiceName),
    ShuttingDownInitiated,
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
}
