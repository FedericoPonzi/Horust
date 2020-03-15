use crate::horust::formats::{RestartStrategy, Service, ServiceName, ServiceStatus};
use nix::unistd::Pid;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceHandler {
    service: Service,
    pub(crate) status: ServiceStatus,
    pub(crate) pid: Option<Pid>,
    // Used only by the Runtime.
    pub marked_for_killing: bool,
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
            marked_for_killing: false,
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

    pub fn shutting_down_started(&mut self) {
        self.marked_for_killing = false;
        self.shutting_down_start = Some(Instant::now());
    }

    pub fn set_status_by_exit_code(&mut self, exit_code: i32) {
        self.shutting_down_start = None;
        let has_failed = !self
            .service
            .failure
            .successfull_exit_code
            .contains(&exit_code);
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
