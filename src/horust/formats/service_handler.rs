use crate::horust::formats::{Service, ServiceName, ServiceStatus};
use nix::unistd::Pid;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceHandler {
    service: Service,
    pub(crate) status: ServiceStatus,
    pub(crate) pid: Option<Pid>,
    pub(crate) restart_attempts: u32,
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
        debug!(
            "Name: {}, Old status: {}, New status: {}",
            self.name(),
            self.status,
            status
        );

        self.status = status;
    }

    pub fn is_to_be_run(&self) -> bool {
        self.status == ServiceStatus::ToBeRun
    }

    pub fn is_failed(&self) -> bool {
        self.status == ServiceStatus::Failed
    }

    pub fn restart_attempts_are_over(&self) -> bool {
        self.restart_attempts > self.service.restart.attempts
    }

    pub fn is_finished_failed(&self) -> bool {
        self.status == ServiceStatus::FinishedFailed
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
        ServiceStatus::Finished == self.status
    }

    pub fn shutting_down_started(&mut self) {
        self.shutting_down_start = Some(Instant::now());
        self.status = ServiceStatus::InKilling;
    }
}
