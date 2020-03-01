use crate::horust::formats::{Event, RestartStrategy, Service, ServiceStatus, UpdatesQueue};
use nix::unistd::Pid;
use std::sync::{Arc, Mutex};

use std::time::Instant;

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
    pub fn ingest(&mut self, name: &str) {
        let mut updates: Vec<Event> = self.updates_queue.receiver.try_iter().collect();
        //debug!("Received the following updatees: {:?}", updates);
        self.services.iter_mut().for_each(|sh| {
            updates = updates
                .clone()
                .into_iter()
                .filter_map(|event| {
                    match &event {
                        Event::StatusChanged(changed) => {
                            if changed.name() == sh.name() {
                                sh.status = changed.status.clone();
                                return None;
                            }
                        }
                        Event::PidChanged(changed) => {
                            debug!("{}: Received changed pid for: {:?}", name, changed);
                            if changed.name() == sh.name() {
                                sh.pid = changed.pid;
                                return None;
                            } else {
                                println!("It looks like {} !== {}", changed.name(), sh.name());
                            }
                        }
                        _ => return Some(event),
                    }
                    Some(event)
                })
                .collect();
        });
    }

    pub fn update_status_by_exit_code(&mut self, pid: &Pid, exit_code: i32) -> bool {
        let queues = &self.updates_queue;
        let result = self
            .services
            .iter_mut()
            .skip_while(|sh| sh.pid() != Some(pid))
            .take(1)
            .map(|sh| {
                sh.set_status_by_exit_code(exit_code);
                queues.send_updated_status(sh);
            })
            .count();
        result == 1
    }

    pub fn update_pid(&mut self, service_name: String, pid: Pid) {
        let queue = &self.updates_queue;
        self.services
            .iter_mut()
            .filter(|sh| sh.name() == service_name)
            .for_each(|sh| {
                sh.set_pid(pid);
                queue.send_update_pid(sh);
            });
    }

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

    pub fn find_by_pid(&mut self, pid: &Pid) -> Option<&mut ServiceHandler> {
        self.services
            .iter_mut()
            .skip_while(|sh| sh.pid() != Some(pid))
            .take(1)
            .last()
    }

    pub fn is_any_service_running(&self) -> bool {
        self.services.iter().any(|sh| sh.is_running())
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
    status: ServiceStatus,
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
    #[cfg(test)]
    pub fn status(&self) -> &ServiceStatus {
        &self.status
    }
    pub fn name(&self) -> &str {
        self.service.name.as_str()
    }
    pub fn pid(&self) -> Option<&Pid> {
        self.pid.as_ref()
    }
    pub fn set_pid(&mut self, pid: Pid) {
        self.status = ServiceStatus::Starting;
        self.pid = Some(pid);
    }

    /// TODO: set validation of the FSM.
    pub fn set_status(&mut self, status: ServiceStatus) {
        self.status = status;
    }

    pub fn is_to_be_run(&self) -> bool {
        self.status == ServiceStatus::ToBeRun
    }

    pub fn is_failed(&self) -> bool {
        self.status == ServiceStatus::Failed
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
        let has_failed = exit_code != 0;
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
