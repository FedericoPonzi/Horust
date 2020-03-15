mod service;
mod service_handler;
use nix::unistd::Pid;
pub use service::*;
pub use service_handler::ServiceHandler;

#[derive(Debug, Clone)]
pub struct Event {
    pub(crate) service_name: ServiceName,
    pub(crate) kind: EventKind,
}

impl Event {
    pub(crate) fn new(service_name: ServiceName, kind: EventKind) -> Self {
        Self { service_name, kind }
    }
}

#[derive(Debug, Clone)]
pub enum EventKind {
    PidChanged(Pid),
    StatusChanged(ServiceStatus),
    ServiceExited(i32),
    MarkedForKillingChanged(bool),
    //ServiceCreated(ServiceHandler),
}
