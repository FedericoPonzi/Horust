use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, EventKind, ServiceHandler, ServiceName, ServiceStatus};
use nix::unistd::Pid;

/// This struct hides the internal datastructures and operations on the service handlers.
/// It also handle the communication channel with the updates queue, by sending out all the change requests.
/// It can be freely cloned across threads.
#[derive(Debug, Clone)]
pub struct ServiceRepository {
    pub services: Vec<ServiceHandler>,
    pub(crate) updates_queue: BusConnector,
}

impl ServiceRepository {
    pub fn new<T: Into<ServiceHandler>>(services: Vec<T>, updates_queue: BusConnector) -> Self {
        ServiceRepository {
            services: services.into_iter().map(Into::into).collect(),
            updates_queue,
        }
    }

    //TODO: probably this should be failedfinished.
    pub(crate) fn get_failed(&self) -> Vec<ServiceHandler> {
        self.services
            .iter()
            .filter(|sh| sh.is_failed())
            .cloned()
            .collect()
    }

    fn update_from_events(&mut self, events: Vec<Event>) {
        events.into_iter().for_each(|ev| {
            self.services
                .iter_mut()
                .filter(|sh| ev.service_name == *sh.service().name)
                .for_each(|sh| match &ev.kind {
                    EventKind::StatusChanged(status) => {
                        sh.status = status.clone();
                    }
                    EventKind::MarkedForKillingChanged(mark) => {
                        sh.marked_for_killing = *mark;
                    }
                    EventKind::ServiceExited(exit_code) => sh.set_status_by_exit_code(*exit_code),
                    EventKind::PidChanged(pid) => {
                        sh.pid = Some(pid.clone());
                        sh.status = ServiceStatus::Running;
                    }
                })
        });
    }

    /// Process all the received services changes. Non-blocking
    pub fn ingest(&mut self, _name: &str) {
        let updates: Vec<Event> = self.updates_queue.try_get_events();
        //debug!("{}: Received the following updates: {:?}", name, updates);
        self.update_from_events(updates);
    }

    // Adds a pid to a service, and sends an update to other components
    pub fn update_pid(&mut self, service_name: ServiceName, pid: Pid) {
        let queue = &self.updates_queue;
        self.services
            .iter_mut()
            .filter(|sh| *sh.name() == *service_name)
            .for_each(|sh| {
                sh.set_pid(pid);
                queue.send_updated_pid(sh);
            });
    }

    // Changes the status of a services, and sends an update to other components
    pub fn update_status(&mut self, service_name: &str, status: ServiceStatus) {
        let queue = &self.updates_queue;
        self.services
            .iter_mut()
            .filter(|sh| sh.name() == service_name)
            .for_each(|sh| {
                sh.set_status(status.clone());
                queue.send_updated_status(sh);
            });
    }

    pub fn all_finished(&self) -> bool {
        self.services
            .iter()
            .all(|sh| sh.is_finished() || sh.is_failed())
    }

    pub fn get_dependents(&self, name: ServiceName) -> Vec<ServiceHandler> {
        self.services
            .iter()
            .filter(|sh| sh.service().start_after.contains(&name))
            .cloned()
            .collect()
    }

    pub(crate) fn is_service_runnable(&self, sh: &ServiceHandler) -> bool {
        if !sh.is_initial() {
            return false;
        }
        //TODO: check if it's finished failed. Apply restart policy.

        for service_name in sh.start_after() {
            let is_started = self.services.iter().any(|service| {
                service.name() == service_name && (service.is_running() || service.is_finished())
            });
            if !is_started {
                return false;
            }
        }
        true
    }

    // apply a function to all services, and send an update on the bus for the changed services.
    pub fn mutate_service_status_apply<F>(&mut self, fun: F)
    where
        F: FnMut(&mut ServiceHandler) -> Option<&ServiceHandler>,
    {
        let queues = &self.updates_queue;
        self.services
            .iter_mut()
            .map(fun)
            .filter_map(|v| v)
            .for_each(|val| queues.send_updated_status(val))
    }

    // apply a function to all services, and send an update on the bus for the changed service.
    pub fn mutate_service_status(&mut self, modified: Option<&ServiceHandler>) {
        let queues = &self.updates_queue;
        if let Some(val) = modified {
            self.services = self
                .services
                .clone()
                .into_iter()
                .filter(|sh| sh.name() != val.name())
                .chain(vec![val.clone()])
                .collect();
            queues.send_updated_status(val)
        }
    }

    pub fn mutate_marked_for_killing(&mut self, modified: Option<&ServiceHandler>) {
        let queues = &self.updates_queue;
        if let Some(val) = modified {
            self.services = self
                .services
                .clone()
                .into_iter()
                .filter(|sh| sh.name() != val.name())
                .chain(vec![val.clone()])
                .collect();
            queues.send_updated_marked_for_killing(val)
        }
    }
}
