mod service;
mod service_handler;
pub use service::*;
pub use service_handler::ServiceHandler;

#[derive(Debug, Clone)]
pub struct Event {
    pub(crate) service_handler: ServiceHandler,
    pub(crate) kind: EventKind,
}

impl Event {
    pub fn new(service_handler: ServiceHandler, kind: EventKind) -> Self {
        Event {
            service_handler,
            kind,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EventKind {
    StatusChanged,
    PidChanged,
    MarkedForKillingChanged,
    //ServiceCreated(ServiceHandler),
}
