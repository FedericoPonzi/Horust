use crate::horust::bus::BusConnector;
use crate::horust::formats::{Service, ServiceName, ServiceStatus};
use crate::horust::Event;
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub(crate) struct Repo {
    bus: BusConnector<Event>,
    pub(crate) services: HashMap<ServiceName, Service>,
    /// Keep track of services which have a pid and need check for progressing to the running state
    pub(crate) started: HashSet<ServiceName>,
    /// Keep track of running services
    pub(crate) running: HashSet<ServiceName>,
    pub(crate) is_shutting_down: bool,
}

impl Repo {
    pub(crate) fn new(bus: BusConnector<Event>, services: Vec<Service>) -> Self {
        Self {
            bus,
            services: services
                .into_iter()
                .map(|service| (service.name.clone(), service))
                .collect(),
            started: Default::default(),
            running: Default::default(),
            is_shutting_down: false,
        }
    }

    pub(crate) fn ingest(&mut self) {
        self.bus
            .try_get_events()
            .into_iter()
            .for_each(|ev| self.apply(ev))
    }

    /// This is merely updating the local view of the system. No business logic applied.
    fn apply(&mut self, ev: Event) {
        debug!("received ev: {:?}", ev);
        match ev {
            Event::StatusChanged(service_name, new_status) => {
                if new_status == ServiceStatus::Started {
                    self.started.insert(service_name);
                } else if new_status == ServiceStatus::Running {
                    self.started.remove(&service_name);
                    self.running.insert(service_name);
                } else if vec![
                    ServiceStatus::Finished,
                    ServiceStatus::FinishedFailed,
                    ServiceStatus::Failed,
                    ServiceStatus::Finished,
                ]
                .contains(&new_status)
                {
                    // If a service is finished, we don't need to check anymore
                    self.started.remove(&service_name);
                    self.running.remove(&service_name);
                }
            }
            Event::ShuttingDownInitiated => {
                self.is_shutting_down = true;
            }
            Event::ForceKill(service_name) | Event::Kill(service_name) => {
                // If a service is killed, we don't need to check it anymore
                self.started.remove(&service_name);
                self.running.remove(&service_name);
            }
            _ => (),
        }
    }

    pub(crate) fn send_ev(&mut self, ev: Event) {
        self.bus.send_event(ev)
    }
}
