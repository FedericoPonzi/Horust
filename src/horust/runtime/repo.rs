use crate::horust::bus::BusConnector;
use crate::horust::formats::{Service, ServiceName};
use crate::horust::runtime::service_handler::ServiceHandler;
use crate::horust::Event;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) struct Repo {
    // TODO: make it a map ServiceName: ServiceHandler
    pub services: HashMap<ServiceName, ServiceHandler>,
    pub(crate) bus: BusConnector<Event>,
}

impl Repo {
    pub(crate) fn new(bus: BusConnector<Event>, services: Vec<Service>) -> Self {
        let services = services
            .into_iter()
            .map(|service| (service.name.clone(), service.into()))
            .collect();
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

    pub fn all_have_finished(&self) -> bool {
        self.services
            .iter()
            .all(|(_s_name, sh)| sh.is_finished() || sh.is_finished_failed())
    }

    /// Get a mutable reference to the Service Handler
    pub fn get_mut_sh(&mut self, service_name: &ServiceName) -> &mut ServiceHandler {
        self.services.get_mut(service_name).unwrap()
    }

    /// Get an immutable reference to the Service Handler
    pub fn get_sh(&mut self, service_name: &ServiceName) -> &ServiceHandler {
        self.services.get(service_name).unwrap()
    }

    /// Get all the services that have specifed "start-after = [`service_name`]" in their config
    pub(crate) fn get_dependents(&self, service_name: &ServiceName) -> Vec<ServiceName> {
        self.services
            .iter()
            .filter(|(_s_name, sh)| sh.service().start_after.contains(service_name))
            .map(|(s_name, _sh)| s_name)
            .cloned()
            .collect()
    }

    /// Get all the services that have specified "die-if-failed = [`service_name`]" in their config
    pub(crate) fn get_die_if_failed(&self, service_name: &ServiceName) -> Vec<&ServiceName> {
        self.services
            .iter()
            .filter(|(_s_name, sh)| {
                sh.service()
                    .termination
                    .die_if_failed
                    .contains(service_name)
            })
            .map(|(s_name, _sh)| s_name)
            .collect()
    }

    pub(crate) fn send_ev(&mut self, ev: Event) {
        self.bus.send_event(ev)
    }

    /// Checks if the service is runnable. So the current status is Initial, and
    /// all the start-after have started or finished.
    pub(crate) fn is_service_runnable(&self, sh: &ServiceHandler) -> bool {
        if !sh.is_initial() {
            return false;
        }
        let is_started = |service_name: &ServiceName| {
            let sh = self.services.get(service_name).unwrap();
            sh.is_running() || sh.is_finished()
        };
        sh.start_after().iter().all(is_started)
    }

    pub(crate) fn any_finished_failed(&self) -> bool {
        self.services
            .iter()
            .any(|(_s_name, sh)| sh.is_finished_failed())
    }
}
