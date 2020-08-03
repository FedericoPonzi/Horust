use crate::horust::formats::{
    FailureStrategy, HealthinessStatus, RestartStrategy, Service, ServiceName, ServiceStatus,
};
use crate::horust::supervisor::repo::Repo;
use crate::horust::Event;
use nix::unistd::Pid;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ServiceHandler {
    service: Service,
    pub(crate) status: ServiceStatus,
    pub(crate) pid: Option<Pid>,
    pub(crate) restart_attempts: u32,
    pub(crate) healthiness_checks_failed: Option<i32>,
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
            healthiness_checks_failed: None,
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

    pub(crate) fn is_early_state(&self) -> bool {
        const EARLY_STATES: [ServiceStatus; 3] = [
            ServiceStatus::Initial,
            ServiceStatus::Starting,
            ServiceStatus::Started,
        ];
        EARLY_STATES.contains(&self.status)
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

    pub fn next(&self, repo: &Repo, is_shutting_down: bool) -> Vec<Event> {
        next(self, repo, is_shutting_down)
    }
    pub fn change_status(&self, new_status: ServiceStatus) -> (ServiceHandler, ServiceStatus) {
        handle_status_change(&self, new_status)
    }

    pub fn restart_attempts_are_over(&self) -> bool {
        self.service.restart.attempts == 0 || self.restart_attempts > self.service.restart.attempts
    }
    pub fn add_healthcheck_event(&mut self, check: HealthinessStatus) {
        let old_val = self.healthiness_checks_failed.unwrap_or(0);
        let new_val = old_val
            + if vec![
                ServiceStatus::Running,
                ServiceStatus::Started,
                ServiceStatus::Starting,
            ]
            .contains(&self.status)
            {
                if let HealthinessStatus::Healthy = check {
                    0
                } else {
                    warn!("{}: healthchecks failed: {}", self.name(), old_val);
                    1
                }
            } else {
                0
            };
        self.healthiness_checks_failed = Some(new_val);
    }

    pub fn is_finished_failed(&self) -> bool {
        ServiceStatus::FinishedFailed == self.status
    }

    pub fn is_in_killing(&self) -> bool {
        ServiceStatus::InKilling == self.status
    }

    /// Returns true if the last few events of the healthchecker were Unhealthy events.
    pub fn has_some_failed_healthchecks(&self) -> bool {
        // If health status message didn't reach the service handler on time, it has failed too fast
        // So we consider it as unhealthy (thus the `1` for the unwrap).
        self.healthiness_checks_failed.unwrap_or(1) > 0
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
    }
}

/// Generates events that, if applied, will make service_handler FSM progress
pub(crate) fn next(
    service_handler: &ServiceHandler,
    repo: &Repo,
    is_shutting_down: bool,
) -> Vec<Event> {
    if is_shutting_down {
        next_events_shutting_down(service_handler)
    } else {
        next_events(repo, service_handler)
    }
}

/// Generate the events needed for moving forward the FSM for the service handler
/// If the system is shutting down, it will call next_shutting_down.
fn next_events(repo: &Repo, service_handler: &ServiceHandler) -> Vec<Event> {
    let ev_status =
        |status: ServiceStatus| Event::new_status_update(service_handler.name(), status);
    let vev_status = |status: ServiceStatus| vec![ev_status(status)];
    match service_handler.status {
        ServiceStatus::Initial if repo.is_service_runnable(&service_handler) => {
            vec![Event::Run(service_handler.name().clone())]
        }
        // if enough time have passed, this will be considered running
        ServiceStatus::Started if !service_handler.has_some_failed_healthchecks() => {
            vev_status(ServiceStatus::Running)
        }
        // This will kill the service after 3 failed healthchecks in a row.
        // Maybe this should be parametrized
        ServiceStatus::Running if service_handler.healthiness_checks_failed.unwrap_or(-1) > 2 => {
            vec![
                ev_status(ServiceStatus::InKilling),
                Event::Kill(service_handler.name().clone()),
            ]
        }
        ServiceStatus::Success => vec![handle_restart_strategy(service_handler, false)],
        ServiceStatus::Failed => {
            let mut failure_evs = handle_failed_service(
                repo.get_dependents(service_handler.name()),
                service_handler.service(),
            );
            let other_services_termination = repo
                .get_die_if_failed(service_handler.name())
                .into_iter()
                .map(|sh_name| {
                    vec![
                        Event::new_status_update(sh_name, ServiceStatus::InKilling),
                        Event::Kill(sh_name.clone()),
                    ]
                })
                .flatten();

            let service_ev = handle_restart_strategy(service_handler, true);

            failure_evs.push(service_ev);
            failure_evs.extend(other_services_termination);
            failure_evs
        }
        ServiceStatus::InKilling if should_force_kill(service_handler) => vec![
            Event::new_force_kill(service_handler.name()),
            Event::new_status_changed(service_handler.name(), ServiceStatus::Failed),
        ],

        _ => vec![],
    }
}

/// This next function assumes that the system is shutting down.
/// It will make progress in the direction of shutting everything down.
fn next_events_shutting_down(service_handler: &ServiceHandler) -> Vec<Event> {
    let ev_status =
        |status: ServiceStatus| Event::new_status_update(service_handler.name(), status);
    let vev_status = |status: ServiceStatus| vec![ev_status(status)];

    // Handle the new state separately if we're shutting down.
    match service_handler.status {
        ServiceStatus::Running | ServiceStatus::Started => vec![
            ev_status(ServiceStatus::InKilling),
            Event::Kill(service_handler.name().clone()),
        ],
        ServiceStatus::Success | ServiceStatus::Initial => vev_status(ServiceStatus::Finished),
        ServiceStatus::Failed => vev_status(ServiceStatus::FinishedFailed),
        ServiceStatus::InKilling if should_force_kill(service_handler) => {
            vec![Event::new_force_kill(service_handler.name())]
        }
        _ => vec![],
    }
}

// TODO: test
/// Handles the service handler's status change
fn handle_status_change(
    service_handler: &ServiceHandler,
    next_status: ServiceStatus,
) -> (ServiceHandler, ServiceStatus) {
    let mut new_service_handler = service_handler.clone();
    if next_status == service_handler.status {
        return (new_service_handler, next_status);
    }
    //TODO: refactor + cleanup.
    // A -> [B,C] means that transition to A is allowed only if service is in state B or C.
    let allowed_transitions = hashmap! {
        ServiceStatus::Initial        => vec![ServiceStatus::Success, ServiceStatus::Failed,
                                              ServiceStatus::Started],
        ServiceStatus::Starting       => vec![ServiceStatus::Initial],
        ServiceStatus::Started        => vec![ServiceStatus::Starting],
        ServiceStatus::InKilling      => vec![ServiceStatus::Initial,
                                              ServiceStatus::Running,
                                              ServiceStatus::Starting,
                                              ServiceStatus::Started],
        ServiceStatus::Running        => vec![ServiceStatus::Started],
        ServiceStatus::FinishedFailed => vec![ServiceStatus::Starting,
                                              ServiceStatus::Started,
                                              ServiceStatus::Failed,
                                              ServiceStatus::InKilling],
        ServiceStatus::Success        => vec![ServiceStatus::Starting,
                                              ServiceStatus::Started,
                                              ServiceStatus::Running,
                                              ServiceStatus::InKilling],
        ServiceStatus::Failed         => vec![ServiceStatus::Starting,
                                              ServiceStatus::Started,
                                              ServiceStatus::Running,
                                              ServiceStatus::InKilling],
        ServiceStatus::Finished       => vec![ServiceStatus::Success,
                                             ServiceStatus::Initial],
    };
    let allowed = allowed_transitions
        .get(&next_status)
        .unwrap_or_else(|| panic!("New status: {} not found!", next_status));
    if allowed.contains(&service_handler.status) {
        match next_status {
            ServiceStatus::Started if allowed.contains(&service_handler.status) => {
                new_service_handler.status = ServiceStatus::Started;
                new_service_handler.restart_attempts = 0;
            }
            ServiceStatus::Running if allowed.contains(&service_handler.status) => {
                new_service_handler.status = ServiceStatus::Running;
            }
            ServiceStatus::InKilling if allowed.contains(&service_handler.status) => {
                debug!(
                    " service: {},  status: {}, new status: {}",
                    service_handler.name(),
                    service_handler.status,
                    next_status
                );
                new_service_handler.status = if service_handler.status == ServiceStatus::Initial {
                    ServiceStatus::Success
                } else {
                    ServiceStatus::InKilling
                };
            }
            new_status => {
                new_service_handler.status = new_status;
            }
        }
    } else {
        debug!(
            "Tried to make an illegal transition: (current) {} ⇾ {} (received) for service: {}",
            service_handler.status,
            next_status,
            service_handler.name()
        );
    }
    let new_status = new_service_handler.status.clone();
    (new_service_handler, new_status)
}

/// Produces events based on the Restart Strategy of the service.
fn handle_restart_strategy(service_handler: &ServiceHandler, is_failed: bool) -> Event {
    let new_status = match service_handler.service.restart.strategy {
        RestartStrategy::Never if is_failed => {
            debug!(
                "restart attemps: {}, are over: {}, max: {}",
                service_handler.restart_attempts,
                service_handler.restart_attempts_are_over(),
                service_handler.service.restart.attempts
            );
            if service_handler.restart_attempts_are_over() {
                ServiceStatus::FinishedFailed
            } else {
                ServiceStatus::Initial
            }
        }
        RestartStrategy::OnFailure if is_failed => ServiceStatus::Initial,
        RestartStrategy::Never | RestartStrategy::OnFailure => ServiceStatus::Finished,
        RestartStrategy::Always => ServiceStatus::Initial,
    };
    debug!("Restart strategy applied, ev: {:?}", new_status);
    Event::new_status_update(service_handler.name(), new_status)
}

/// This is applied to both failed and FinishedFailed services.
fn handle_failed_service(deps: Vec<ServiceName>, failed_sh: &Service) -> Vec<Event> {
    match failed_sh.failure.strategy {
        FailureStrategy::Shutdown => vec![Event::ShuttingDownInitiated],
        FailureStrategy::KillDependents => {
            debug!("Failed service has kill-dependents strategy, going to mark them all..");
            deps.iter()
                .map(|sh| {
                    vec![
                        Event::new_status_update(sh, ServiceStatus::InKilling),
                        Event::Kill(sh.clone()),
                    ]
                })
                .flatten()
                .collect()
        }
        FailureStrategy::Ignore => vec![],
    }
}

/// Check if we've waitied enough for the service to exit
fn should_force_kill(service_handler: &ServiceHandler) -> bool {
    if service_handler.pid.is_none() {
        // Since it was in the started state, it doesn't have a pid yet.
        // Let's give it the time to start and exit.
        return false;
    }
    if let Some(shutting_down_elapsed_secs) = service_handler.shutting_down_start {
        let shutting_down_elapsed_secs = shutting_down_elapsed_secs.elapsed().as_secs();
        debug!(
            "{}, should not force kill. Elapsed: {}, termination wait: {}",
            service_handler.name(),
            shutting_down_elapsed_secs,
            service_handler.service().termination.wait.clone().as_secs()
        );
        shutting_down_elapsed_secs > service_handler.service().termination.wait.clone().as_secs()
    } else {
        // this might happen, because InKilling state is emitted before the Kill event.
        // So maybe the supervisor has received only the InKilling state change, but hasn't sent the
        // signal yet. So it should be fine.
        debug!("There is no shutting down elapsed secs.");
        false
    }
}

#[cfg(test)]
mod test {
    use crate::horust::formats::{FailureStrategy, Service, ServiceStatus};
    use crate::horust::supervisor::service_handler::{
        handle_failed_service, handle_restart_strategy, should_force_kill, ServiceHandler,
    };
    use crate::horust::Event;
    use nix::unistd::Pid;
    use std::ops::Sub;
    use std::str::FromStr;
    use std::time::Duration;

    #[test]
    fn test_handle_restart_strategy() {
        let new_status = |status| Event::new_status_update(&"servicename".to_string(), status);
        let matrix = vec![
            (false, "always", new_status(ServiceStatus::Initial)),
            (true, "always", new_status(ServiceStatus::Initial)),
            (true, "on-failure", new_status(ServiceStatus::Initial)),
            (false, "on-failure", new_status(ServiceStatus::Finished)),
            (true, "never", new_status(ServiceStatus::FinishedFailed)),
            (false, "never", new_status(ServiceStatus::Finished)),
        ];
        matrix
            .into_iter()
            .for_each(|(has_failed, strategy, expected)| {
                let service = format!(
                    r#"name="servicename"
command = "Not relevant"
[restart]
strategy = "{}"
"#,
                    strategy
                );
                let service: Service = Service::from_str(service.as_str()).unwrap();
                let sh = service.into();
                let received = handle_restart_strategy(&sh, has_failed);
                assert_eq!(received, expected);
            });
    }

    #[test]
    fn test_should_force_kill() {
        let service = r#"command="notrelevant"
[termination]
wait = "10s"
"#;
        let service: Service = toml::from_str(service).unwrap();
        let mut sh: ServiceHandler = service.into();
        assert!(!should_force_kill(&sh));
        sh.shutting_down_started();
        sh.status = ServiceStatus::InKilling;
        assert!(!should_force_kill(&sh));
        let old_start = sh.shutting_down_start;
        let past_wait = Some(sh.shutting_down_start.unwrap().sub(Duration::from_secs(20)));
        sh.shutting_down_start = past_wait;
        assert!(!should_force_kill(&sh));
        sh.pid = Some(Pid::this());
        sh.shutting_down_start = old_start;
        assert!(!should_force_kill(&sh));
        sh.shutting_down_start = past_wait;
        assert!(should_force_kill(&sh));
    }

    #[test]
    fn test_handle_failed_service() {
        let mut service = Service::from_name("b");
        let evs = handle_failed_service(vec!["a".into()], &service.clone());
        assert!(evs.is_empty());

        service.failure.strategy = FailureStrategy::KillDependents;
        let evs = handle_failed_service(vec!["a".into()], &service.clone());
        let exp = vec![
            Event::new_status_update(&"a".to_string(), ServiceStatus::InKilling),
            Event::Kill("a".into()),
        ];
        assert_eq!(evs, exp);

        service.failure.strategy = FailureStrategy::Shutdown;
        let evs = handle_failed_service(vec!["a".into()], &service.into());
        let exp = vec![Event::ShuttingDownInitiated];
        assert_eq!(evs, exp);
    }
}
