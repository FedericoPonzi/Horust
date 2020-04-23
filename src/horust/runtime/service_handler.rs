use crate::horust::formats::{Service, ServiceName, ServiceStatus};
use nix::unistd::Pid;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ServiceHandler {
    service: Service,
    pub(crate) status: ServiceStatus,
    pub(crate) pid: Option<Pid>,
    pub(crate) restart_attempts: u32,
    pub(crate) healthiness_checks_failed: u32,
    /// Instant representing at which time we received a shutdown request. Will be used for comparing Service.termination.wait
    pub(crate) shutting_down_start: Option<Instant>,
}

impl From<Service> for ServiceHandler {
    fn from(service: Service) -> Self {
        ServiceHandler {
            service,
            status: ServiceStatus::Initial,
            pid: None,
            shutting_down_start: None,
            restart_attempts: 0,
            healthiness_checks_failed: 1,
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

    pub fn restart_attempts_are_over(&self) -> bool {
        self.restart_attempts > self.service.restart.attempts
    }

    pub fn is_finished_failed(&self) -> bool {
        ServiceStatus::FinishedFailed == self.status
    }

    pub fn is_in_killing(&self) -> bool {
        ServiceStatus::InKilling == self.status
    }

    pub fn is_starting(&self) -> bool {
        ServiceStatus::Started == self.status
    }

    pub fn is_initial(&self) -> bool {
        ServiceStatus::Initial == self.status
    }

    pub fn is_running(&self) -> bool {
        ServiceStatus::Running == self.status
    }

    pub fn is_finished(&self) -> bool {
        ServiceStatus::Finished == self.status
    }
    pub fn shutting_down_started(&mut self) {
        self.shutting_down_start = Some(Instant::now());
        self.status = ServiceStatus::InKilling;
    }
    pub fn is_started(&self) -> bool {
        ServiceStatus::Started == self.status
    }
}
