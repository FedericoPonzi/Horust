use crate::horust::formats::{RestartStrategy, Service, ServiceStatus};
use nix::unistd::Pid;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub(crate) type Services = Arc<ServiceRepository>;

#[derive(Debug)]
pub(crate) struct ServiceRepository(pub Mutex<Vec<ServiceHandler>>);

impl ServiceRepository {
    pub(crate) fn new<T: Into<ServiceHandler>>(services: Vec<T>) -> Self {
        ServiceRepository(Mutex::new(services.into_iter().map(Into::into).collect()))
    }
    pub(crate) fn set_pid(&self, service_name: String, pid: Pid) {
        self.0
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|sh| sh.name() == service_name)
            .for_each(|sh| {
                sh.set_pid(pid);
            });
    }
    pub(crate) fn set_status(&self, service_name: String, status: ServiceStatus) {
        self.0
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|sh| sh.name() == service_name)
            .for_each(|sh| {
                sh.set_status(status.clone());
            });
    }
    pub(crate) fn is_any_service_running(&self) -> bool {
        self.0.lock().unwrap().iter().any(|sh| sh.is_running())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ServiceHandler {
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
    pub(crate) fn start_after(&self) -> &Vec<String> {
        self.service.start_after.as_ref()
    }
    pub(crate) fn service(&self) -> &Service {
        &self.service
    }
    #[cfg(test)]
    pub(crate) fn status(&self) -> &ServiceStatus {
        &self.status
    }
    pub(crate) fn name(&self) -> &str {
        self.service.name.as_str()
    }
    pub(crate) fn pid(&self) -> Option<&Pid> {
        self.pid.as_ref()
    }
    pub(crate) fn set_pid(&mut self, pid: Pid) {
        self.status = ServiceStatus::Starting;
        self.pid = Some(pid);
    }

    /// TODO: set validation of the FSM.
    pub(crate) fn set_status(&mut self, status: ServiceStatus) {
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
