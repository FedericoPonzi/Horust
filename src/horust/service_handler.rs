use crate::horust::dispatcher::UpdatesQueue;
use crate::horust::formats::{RestartStrategy, Service, ServiceName, ServiceStatus};
use nix::unistd::Pid;
use std::time::Instant;

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
    //ServiceCreated(ServiceHandler),
}

/// This struct hides the internal datastructures and operations on the service handlers.
/// It also handle the communication channel with the updates queue, by sending out all the change requests.
/// It can be freely cloned across threads.
#[derive(Debug, Clone)]
pub struct ServiceRepository {
    pub services: Vec<ServiceHandler>,
    updates_queue: UpdatesQueue,
}

impl ServiceRepository {
    pub fn new<T: Into<ServiceHandler>>(services: Vec<T>, updates_queue: UpdatesQueue) -> Self {
        ServiceRepository {
            services: services.into_iter().map(Into::into).collect(),
            updates_queue,
        }
    }

    /// Process all the received services changes.
    pub fn ingest(&mut self, _name: &str) {
        let mut updates: Vec<Event> = self.updates_queue.try_get_events();
        if !updates.is_empty() {
            //debug!("{}: Received the following updates: {:?}", name, updates);
            self.services.iter_mut().for_each(|sh| {
                updates = updates
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
                                    sh.status = ev.service_handler.status.clone();
                                    sh.pid = ev.service_handler.pid;
                                }
                            }
                        }
                        // If this event has been consumed (e.g. shname == ev.service_name) thne I can just throw it away..
                        !to_consume
                    })
                    .collect();
            });
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

    pub fn get_runnable_services(&self) -> Vec<ServiceHandler> {
        let check_can_run = |sh: &ServiceHandler| {
            if sh.is_initial() {
                return true;
            }
            let mut check_run = false;
            for service_name in sh.start_after() {
                for service in self.services.iter() {
                    let is_started = service.name() == service_name
                        && (service.is_running() || service.is_finished());
                    if is_started {
                        check_run = true;
                    }
                }
            }
            check_run
        };
        self.services
            .iter()
            .cloned()
            .filter(|v| check_can_run(v))
            .collect()
    }

    //
    pub fn mutate_service_status<F>(&mut self, fun: F)
    where
        F: FnMut(&mut ServiceHandler) -> Option<&mut ServiceHandler>,
    {
        let queues = &self.updates_queue;
        self.services
            .iter_mut()
            .map(fun)
            .filter_map(|v| v)
            .for_each(|val| queues.send_updated_status(val))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceHandler {
    service: Service,
    pub(crate) status: ServiceStatus,
    pid: Option<Pid>,
    last_state_change: Option<Instant>,
}

impl From<Service> for ServiceHandler {
    fn from(service: Service) -> Self {
        ServiceHandler {
            service,
            status: ServiceStatus::Initial,
            pid: None,
            last_state_change: None,
        }
    }
}

impl From<ServiceHandler> for Service {
    fn from(sh: ServiceHandler) -> Self {
        sh.service
    }
}

impl ServiceHandler {
    pub fn start_after(&self) -> &Vec<String> {
        self.service.start_after.as_ref()
    }

    pub fn service(&self) -> &Service {
        &self.service
    }

    pub fn name(&self) -> &ServiceName {
        &self.service.name
    }

    pub fn pid(&self) -> Option<Pid> {
        self.pid
    }

    pub fn set_pid(&mut self, pid: Pid) {
        self.status = ServiceStatus::Starting;
        self.pid = Some(pid);
    }

    pub fn set_status(&mut self, status: ServiceStatus) {
        self.status = status;
    }

    pub fn is_to_be_run(&self) -> bool {
        self.status == ServiceStatus::ToBeRun
    }

    pub fn is_failed(&self) -> bool {
        self.status == ServiceStatus::Failed
    }

    pub fn is_in_killing(&self) -> bool {
        self.status == ServiceStatus::InKilling
    }

    pub fn is_starting(&self) -> bool {
        self.status == ServiceStatus::Starting
    }

    pub fn is_initial(&self) -> bool {
        self.status == ServiceStatus::Initial
    }

    pub fn is_running(&self) -> bool {
        self.status == ServiceStatus::Running
    }

    pub fn is_finished(&self) -> bool {
        ServiceStatus::Finished == self.status || self.status == ServiceStatus::Failed
    }

    pub fn set_status_by_exit_code(&mut self, exit_code: i32) {
        let has_failed = self.service.failure.exit_code.contains(&exit_code);
        if has_failed {
            error!(
                "Service: {} has failed, exit code: {}",
                self.name(),
                exit_code
            );
        } else {
            info!("Service: {} successfully exited.", self.name());
        }
        match self.service.restart.strategy {
            RestartStrategy::Never => {
                // Will never be restarted, even if failed:
                self.status = if has_failed {
                    ServiceStatus::Failed
                } else {
                    ServiceStatus::Finished
                };
            }
            RestartStrategy::OnFailure => {
                self.status = if has_failed {
                    ServiceStatus::Initial
                } else {
                    ServiceStatus::Finished
                };
                debug!("Going to rerun the process because it failed!");
            }
            RestartStrategy::Always => {
                self.status = ServiceStatus::Initial;
            }
        };
        self.pid = None;
    }
}
