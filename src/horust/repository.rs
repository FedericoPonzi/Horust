use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, EventKind, ServiceHandler, ServiceName, ServiceStatus};
use nix::unistd::Pid;

/// This struct hides the internal datastructures and operations on the service handlers.
/// It also handle the communication channel with the updates queue, by sending out all the change requests.
/// It can be freely cloned across threads.
#[derive(Debug, Clone)]
pub struct ServiceRepository {
    pub services: Vec<ServiceHandler>,
    updates_queue: BusConnector,
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

    fn update_from_events(&mut self, mut events: Vec<Event>) {
        self.services.iter_mut().for_each(|sh| {
            events = events
                .clone()
                .into_iter()
                .filter(|ev| {
                    let to_consume = sh.name() == ev.service_handler.name();
                    if to_consume {
                        match &ev.kind {
                            EventKind::StatusChanged => {
                                sh.status = ev.service_handler.status.clone();
                            }
                            EventKind::PidChanged => {
                                sh.pid = ev.service_handler.pid.clone();
                                sh.status = ev.service_handler.status.clone()
                            }
                        }
                    }
                    // If this event has been consumed (e.g. shname == ev.service_name) thne I can just throw it away..
                    !to_consume
                })
                .collect();
        });
    }

    /// Process all the received services changes. Non-blocking
    pub fn ingest(&mut self, _name: &str) {
        let updates: Vec<Event> = self.updates_queue.try_get_events();
        if !updates.is_empty() {
            //debug!("{}: Received the following updates: {:?}", name, updates);
            self.update_from_events(updates);
        }
    }

    // True if the update was applied, false otherwise.
    pub fn update_status_by_exit_code(&mut self, pid: Pid, exit_code: i32) -> bool {
        let queues = &self.updates_queue;
        for service in self.services.iter_mut() {
            if service.pid() == Some(pid) {
                service.set_status_by_exit_code(exit_code);
                queues.send_updated_status(service);
                return true;
            }
        }
        false
    }
    // Adds a pid to a service, and sends an update to other components
    pub fn update_pid(&mut self, service_name: ServiceName, pid: Pid) {
        let queue = &self.updates_queue;
        self.services
            .iter_mut()
            .filter(|sh| *sh.name() == *service_name)
            .for_each(|sh| {
                sh.set_pid(pid);
                queue.send_update_pid(sh);
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

    pub fn is_any_service_to_be_run(&self) -> bool {
        self.services.iter().any(|sh| sh.is_to_be_run())
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
    pub fn get_in_killing_services(&self) -> Vec<ServiceHandler> {
        self.services
            .iter()
            .filter(|v| v.is_in_killing())
            .cloned()
            .collect()
    }
    pub fn get_runnable_services(&self) -> Vec<ServiceHandler> {
        let check_can_run = |sh: &ServiceHandler| {
            let mut check_run = true;
            for service_name in sh.start_after() {
                for service in self.services.iter() {
                    let is_started = service.name() == service_name
                        && (service.is_running() || service.is_finished());
                    // if not started, then cannot run!
                    check_run = !is_started;
                }
            }
            sh.is_initial() && check_run
        };

        self.services
            .iter()
            .filter(|v| check_can_run(*v))
            .cloned()
            .collect()
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
}
