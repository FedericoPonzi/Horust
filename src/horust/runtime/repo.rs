use crate::horust::bus::BusConnector;
use crate::horust::formats::{ServiceHandler, ServiceName};
use crate::horust::Event;

#[derive(Debug, Clone)]
pub(crate) struct Repo {
    // TODO: make it a map ServiceName: ServiceHandler
    pub services: Vec<ServiceHandler>,
    pub(crate) bus: BusConnector<Event>,
}

impl Repo {
    pub(crate) fn new<T: Into<ServiceHandler>>(bus: BusConnector<Event>, services: Vec<T>) -> Self {
        let services = services.into_iter().map(Into::into).collect();
        Self { bus, services }
    }

    /// Non blocking
    pub(crate) fn get_events(&mut self) -> Vec<Event> {
        self.bus.try_get_events()
    }

    /// Blocking
    pub(crate) fn get_n_events_blocking(&mut self, quantity: usize) -> Vec<Event> {
        self.bus.get_n_events_blocking(quantity)
    }

    pub fn all_finished(&self) -> bool {
        self.services
            .iter()
            .all(|sh| sh.is_finished() || sh.is_finished_failed())
    }

    pub fn get_mut_service(&mut self, service_name: &ServiceName) -> &mut ServiceHandler {
        self.services
            .iter_mut()
            .filter(|sh| sh.name() == service_name)
            .last()
            .unwrap()
    }
    /// Get all the services that have specifed "start-after = [`service_name`]" in their config
    pub(crate) fn get_dependents(&self, service_name: &ServiceName) -> Vec<ServiceName> {
        self.services
            .iter()
            .filter(|sh| sh.service().start_after.contains(service_name))
            .map(|sh| sh.name())
            .cloned()
            .collect()
    }

    pub(crate) fn get_die_if_failed(&self, service_name: &ServiceName) -> Vec<&ServiceName> {
        self.services
            .iter()
            .filter(|sh| {
                sh.service()
                    .termination
                    .die_if_failed
                    .contains(service_name)
            })
            .map(|sh| sh.name())
            .collect()
    }

    pub(crate) fn send_ev(&mut self, ev: Event) {
        self.bus.send_event(ev)
    }

    pub(crate) fn is_service_runnable(&self, sh: &ServiceHandler) -> bool {
        if !sh.is_initial() {
            return false;
        }
        let is_started = |service_name: &ServiceName| {
            self.services.iter().any(|service| {
                service.name() == service_name && (service.is_running() || service.is_finished())
            })
        };
        sh.start_after().iter().all(is_started)
    }
}
