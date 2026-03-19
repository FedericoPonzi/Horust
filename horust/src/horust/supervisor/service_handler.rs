use std::time::Instant;

use nix::unistd::Pid;

use crate::horust::Event;
use crate::horust::formats::{
    FailureStrategy, HealthinessStatus, RestartStrategy, Service, ServiceName, ServiceStatus,
};
use crate::horust::supervisor::repo::Repo;

use super::{LifecycleStatus, ShuttingDown};

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub(crate) struct ServiceHandler {
    service: Service,
    /// Status of this service.
    pub(super) status: ServiceStatus,
    /// Process ID of this service, if any
    pub(super) pid: Option<Pid>,
    /// How many times in a row we failed to start this service
    pub(super) restart_attempts: u32,
    /// Amount of healthiness checks failed, applies only if the service is running
    pub(super) healthiness_checks_failed: Option<i32>,
    /// Instant representing at which time we received a shutdown request. Will be used for comparing Service.termination.wait
    pub(super) shutting_down_start: Option<Instant>,
}

impl From<Service> for ServiceHandler {
    fn from(service: Service) -> Self {
        ServiceHandler {
            service,
            ..Default::default()
        }
    }
}

impl ServiceHandler {
    fn is_alive_state(&self) -> bool {
        const ALIVE_STATES: [ServiceStatus; 3] = [
            ServiceStatus::Running,
            ServiceStatus::Started,
            ServiceStatus::Starting,
        ];
        ALIVE_STATES.contains(&self.status)
    }

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

    pub fn next(&self, repo: &Repo, status: LifecycleStatus) -> Vec<Event> {
        next(self, repo, status)
    }
    pub fn change_status(&self, new_status: ServiceStatus) -> (ServiceHandler, ServiceStatus) {
        handle_status_change(self, new_status)
    }

    /// Restart attempts are over if the attempts field is zero or we already retried enough times.
    pub fn restart_attempts_are_over(&self) -> bool {
        self.service.restart.attempts == 0 || self.restart_attempts > self.service.restart.attempts
    }
    pub fn add_healthcheck_event(&mut self, check: HealthinessStatus) {
        let previous_hc = self.healthiness_checks_failed.unwrap_or(0);
        let new_hc =
            i32::from(self.is_alive_state() && !matches!(check, HealthinessStatus::Healthy));
        self.healthiness_checks_failed = Some(previous_hc + new_hc);
    }

    pub fn is_finished_failed(&self) -> bool {
        matches!(self.status, ServiceStatus::FinishedFailed)
    }

    pub fn is_in_killing(&self) -> bool {
        matches!(self.status, ServiceStatus::InKilling)
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
    lifecycle_status: LifecycleStatus,
) -> Vec<Event> {
    match lifecycle_status {
        LifecycleStatus::Running => next_events(repo, service_handler),
        LifecycleStatus::ShuttingDown(shutting_down) => {
            next_events_shutting_down(service_handler, shutting_down)
        }
    }
}

/// Generate the events needed for moving forward the FSM for the service handler
/// If the system is shutting down, it will call next_shutting_down.
fn next_events(repo: &Repo, service_handler: &ServiceHandler) -> Vec<Event> {
    let ev_status =
        |status: ServiceStatus| Event::new_status_update(service_handler.name(), status);
    let vev_status = |status: ServiceStatus| vec![ev_status(status)];

    match service_handler.status {
        ServiceStatus::Initial if repo.is_service_runnable(service_handler) => {
            vec![Event::Run(service_handler.name().clone())]
        }
        // if enough time has passed, this will be considered running
        ServiceStatus::Started if !service_handler.has_some_failed_healthchecks() => {
            vev_status(ServiceStatus::Running)
        }
        // This will kill the service after 3 failed healthchecks in a row.
        // Maybe this should be parametrized
        ServiceStatus::Running
            if service_handler.healthiness_checks_failed.unwrap_or(-1)
                > service_handler.service.healthiness.max_failed =>
        {
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
                .flat_map(|sh_name| {
                    vec![
                        Event::new_status_update(sh_name, ServiceStatus::InKilling),
                        Event::Kill(sh_name.clone()),
                    ]
                });

            let service_ev = handle_restart_strategy(service_handler, true);

            failure_evs.push(service_ev);
            failure_evs.extend(other_services_termination);
            failure_evs
        }
        ServiceStatus::InKilling if should_force_kill(service_handler, None) => vec![
            Event::new_force_kill(service_handler.name()),
            Event::new_status_changed(service_handler.name(), ServiceStatus::Failed),
        ],

        _ => vec![],
    }
}

/// This next function assumes that the system is shutting down.
/// It will make progress in the direction of shutting everything down.
fn next_events_shutting_down(
    service_handler: &ServiceHandler,
    shutting_down: ShuttingDown,
) -> Vec<Event> {
    let ev_status =
        |status: ServiceStatus| Event::new_status_update(service_handler.name(), status);
    let vev_status = |status: ServiceStatus| vec![ev_status(status)];

    // Handle the new state separately if we're shutting down.
    match &service_handler.status {
        ServiceStatus::Running | ServiceStatus::Started => vec![
            ev_status(ServiceStatus::InKilling),
            Event::Kill(service_handler.name().clone()),
        ],
        ServiceStatus::Success | ServiceStatus::Initial => vev_status(ServiceStatus::Finished),
        ServiceStatus::Failed => vev_status(ServiceStatus::FinishedFailed),
        ServiceStatus::InKilling if should_force_kill(service_handler, shutting_down) => {
            vec![Event::new_force_kill(service_handler.name())]
        }
        _ => vec![],
    }
}

/// Handles the service handler's status change
fn handle_status_change(
    service_handler: &ServiceHandler,
    next_status: ServiceStatus,
) -> (ServiceHandler, ServiceStatus) {
    use ServiceStatus::*;

    let mut new_service_handler = service_handler.clone();
    if next_status == service_handler.status {
        return (new_service_handler, next_status);
    }

    // Static lookup table of valid transitions
    // A -> [B,C] means that transition to A is allowed only if the service is in state B or C.
    const ALLOWED_TRANSITIONS: &[(ServiceStatus, &[ServiceStatus])] = &[
        (Initial, &[Success, Failed]),
        (Starting, &[Initial]),
        (Started, &[Starting]),
        (InKilling, &[Initial, Running, Starting, Started]),
        (Running, &[Started]),
        (FinishedFailed, &[Starting, Started, Failed, InKilling]),
        (Success, &[Starting, Started, Running, InKilling]),
        (Failed, &[Starting, Started, Running, InKilling]),
        (Finished, &[Success, Initial]),
    ];

    let allowed = ALLOWED_TRANSITIONS
        .iter()
        .find(|(status, _)| *status == next_status)
        .map(|(_, allowed_from)| allowed_from);

    let valid = allowed.is_some_and(|allowed_from| allowed_from.contains(&service_handler.status));

    if valid {
        match next_status {
            Started => {
                new_service_handler.status = Started;
                new_service_handler.restart_attempts = 0;
            }
            InKilling if service_handler.status == Initial => {
                // Nothing to do here, the service was never started.
                debug!(
                    " service: {},  status: {}, new status: {}",
                    service_handler.name(),
                    service_handler.status,
                    next_status
                );
                new_service_handler.status = Success;
            }
            new_status => {
                new_service_handler.status = new_status;
            }
        }
    } else {
        error!(
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
                "restart attempts: {}, are over: {}, max: {}",
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
        FailureStrategy::Shutdown => vec![Event::ShuttingDownInitiated(ShuttingDown::Gracefully)],
        FailureStrategy::KillDependents => {
            debug!("Failed service has kill-dependents strategy, going to mark them all..");
            deps.iter()
                .flat_map(|sh| {
                    vec![
                        Event::new_status_update(sh, ServiceStatus::InKilling),
                        Event::Kill(sh.clone()),
                    ]
                })
                .collect()
        }
        FailureStrategy::Ignore => vec![],
    }
}

/// Check if we've waited enough for the service to exit
fn should_force_kill(
    service_handler: &ServiceHandler,
    shutting_down: impl Into<Option<ShuttingDown>>,
) -> bool {
    if service_handler.pid.is_none() {
        // Since it was in the started state, it doesn't have a pid yet.
        // Let's give it the time to start and exit.
        return false;
    }
    if let Some(ShuttingDown::Forcefully) = shutting_down.into() {
        debug!("{}, should force kill.", service_handler.name());
        return true;
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
    use std::ops::Sub;
    use std::str::FromStr;
    use std::time::Duration;

    use nix::unistd::Pid;

    use crate::horust::Event;
    use crate::horust::formats::{FailureStrategy, Service, ServiceStatus, ShuttingDown};
    use crate::horust::supervisor::service_handler::{
        ServiceHandler, handle_failed_service, handle_restart_strategy, should_force_kill,
    };

    #[test]
    fn test_handle_restart_strategy() {
        let new_status = |status| Event::new_status_update("servicename", status);
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
        assert!(!should_force_kill(&sh, None));
        sh.shutting_down_started();
        sh.status = ServiceStatus::InKilling;
        assert!(!should_force_kill(&sh, None));
        let old_start = sh.shutting_down_start;
        let past_wait = Some(sh.shutting_down_start.unwrap().sub(Duration::from_secs(20)));
        sh.shutting_down_start = past_wait;
        assert!(!should_force_kill(&sh, None));
        sh.pid = Some(Pid::this());
        sh.shutting_down_start = old_start;
        assert!(!should_force_kill(&sh, None));
        sh.shutting_down_start = past_wait;
        assert!(should_force_kill(&sh, None));
    }

    #[test]
    fn test_handle_failed_service() {
        let mut service = Service::from_name("b");
        let evs = handle_failed_service(vec!["a".into()], &service);
        assert!(evs.is_empty());

        service.failure.strategy = FailureStrategy::KillDependents;
        let evs = handle_failed_service(vec!["a".into()], &service);
        let exp = vec![
            Event::new_status_update("a", ServiceStatus::InKilling),
            Event::Kill("a".into()),
        ];
        assert_eq!(evs, exp);

        service.failure.strategy = FailureStrategy::Shutdown;
        let evs = handle_failed_service(vec!["a".into()], &service);
        let exp = vec![Event::ShuttingDownInitiated(ShuttingDown::Gracefully)];
        assert_eq!(evs, exp);
    }

    // --- Helper to build a ServiceHandler in a given state ---

    use crate::horust::supervisor::test_utils::{
        make_handler, make_repo, make_repo_from_services, make_repo_with_start_after,
    };

    // ========================================================================
    // State machine transition tests (handle_status_change)
    // ========================================================================

    #[test]
    fn test_valid_transitions() {
        use ServiceStatus::*;

        // (current_status, next_status, expected_resulting_status)
        let valid_cases = vec![
            (Success, Initial, Initial),
            (Failed, Initial, Initial),
            (Initial, Starting, Starting),
            (Starting, Started, Started),
            (Started, Running, Running),
            (Running, Success, Success),
            (Running, Failed, Failed),
            (Success, Finished, Finished),
            (Initial, Finished, Finished),
            // InKilling from various valid sources
            (Running, InKilling, InKilling),
            (Starting, InKilling, InKilling),
            (Started, InKilling, InKilling),
            // FinishedFailed from various sources
            (Starting, FinishedFailed, FinishedFailed),
            (Started, FinishedFailed, FinishedFailed),
            (Failed, FinishedFailed, FinishedFailed),
            (InKilling, FinishedFailed, FinishedFailed),
            // Success/Failed from InKilling
            (InKilling, Success, Success),
            (InKilling, Failed, Failed),
        ];

        for (current, next, expected) in valid_cases {
            let sh = make_handler("svc", current.clone());
            let (new_sh, new_status) = sh.change_status(next.clone());
            assert_eq!(
                new_status, expected,
                "Transition {:?} → {:?} should produce {:?}, got {:?}",
                current, next, expected, new_status
            );
            assert_eq!(new_sh.status, expected);
        }
    }

    #[test]
    fn test_invalid_transitions_leave_status_unchanged() {
        use ServiceStatus::*;

        let invalid_cases = vec![
            (Initial, Running),        // Can't jump to Running directly
            (Initial, Failed),         // Not a valid source for Failed
            (Finished, Starting),      // Terminal state
            (Finished, Initial),       // Terminal state (not listed as valid source for Initial)
            (FinishedFailed, Initial), // Terminal state
            (Running, Initial),        // Can't go backward
            (Running, Starting),       // Can't go backward
            (Running, Started),        // Can't go backward
            (Success, Starting),       // Not a valid source for Starting
        ];

        for (current, next) in invalid_cases {
            let sh = make_handler("svc", current.clone());
            let (_new_sh, new_status) = sh.change_status(next.clone());
            assert_eq!(
                new_status, current,
                "Invalid transition {:?} → {:?} should leave status as {:?}, got {:?}",
                current, next, current, new_status
            );
        }
    }

    #[test]
    fn test_same_status_transition_is_noop() {
        use ServiceStatus::*;
        for status in [
            Initial,
            Starting,
            Started,
            Running,
            InKilling,
            Success,
            Failed,
            Finished,
            FinishedFailed,
        ] {
            let sh = make_handler("svc", status.clone());
            let (new_sh, new_status) = sh.change_status(status.clone());
            assert_eq!(
                new_status,
                status.clone(),
                "Same-status transition for {:?} should be noop",
                status
            );
            assert_eq!(new_sh.status, status);
        }
    }

    #[test]
    fn test_in_killing_from_initial_becomes_success() {
        // Special case: InKilling from Initial → Success (service was never started)
        let sh = make_handler("svc", ServiceStatus::Initial);
        let (new_sh, new_status) = sh.change_status(ServiceStatus::InKilling);
        assert_eq!(new_status, ServiceStatus::Success);
        assert_eq!(new_sh.status, ServiceStatus::Success);
    }

    #[test]
    fn test_started_transition_resets_restart_attempts() {
        let mut sh = make_handler("svc", ServiceStatus::Starting);
        sh.restart_attempts = 5;
        let (new_sh, new_status) = sh.change_status(ServiceStatus::Started);
        assert_eq!(new_status, ServiceStatus::Started);
        assert_eq!(new_sh.restart_attempts, 0);
    }

    #[test]
    fn test_non_started_transition_preserves_restart_attempts() {
        let mut sh = make_handler("svc", ServiceStatus::Started);
        sh.restart_attempts = 3;
        let (new_sh, _) = sh.change_status(ServiceStatus::Running);
        assert_eq!(new_sh.restart_attempts, 3);
    }

    // ========================================================================
    // Healthcheck event tracking tests
    // ========================================================================

    use crate::horust::formats::HealthinessStatus;

    #[test]
    fn test_add_healthcheck_healthy_while_running() {
        let mut sh = make_handler("svc", ServiceStatus::Running);
        sh.add_healthcheck_event(HealthinessStatus::Healthy);
        assert_eq!(sh.healthiness_checks_failed, Some(0));
    }

    #[test]
    fn test_add_healthcheck_event() {
        // Unhealthy while alive increments
        let mut sh = make_handler("svc", ServiceStatus::Running);
        sh.add_healthcheck_event(HealthinessStatus::Unhealthy);
        assert_eq!(sh.healthiness_checks_failed, Some(1));
        sh.add_healthcheck_event(HealthinessStatus::Unhealthy);
        assert_eq!(sh.healthiness_checks_failed, Some(2));

        // Healthy while alive doesn't decrement (just adds 0)
        sh.add_healthcheck_event(HealthinessStatus::Healthy);
        assert_eq!(sh.healthiness_checks_failed, Some(2));

        // Unhealthy while NOT alive (Initial) stays at 0
        let mut sh = make_handler("svc", ServiceStatus::Initial);
        sh.add_healthcheck_event(HealthinessStatus::Unhealthy);
        assert_eq!(sh.healthiness_checks_failed, Some(0));

        // All alive states (Running, Started, Starting) should increment
        for status in [
            ServiceStatus::Running,
            ServiceStatus::Started,
            ServiceStatus::Starting,
        ] {
            let mut sh = make_handler("svc", status.clone());
            sh.add_healthcheck_event(HealthinessStatus::Unhealthy);
            assert_eq!(
                sh.healthiness_checks_failed,
                Some(1),
                "Unhealthy while {:?} should increment",
                status
            );
        }
    }

    #[test]
    fn test_has_some_failed_healthchecks() {
        let sh = make_handler("svc", ServiceStatus::Running);
        // None → unwrap_or(1) > 0 → true (conservative: assume unhealthy)
        assert!(sh.has_some_failed_healthchecks());

        let mut sh = make_handler("svc", ServiceStatus::Running);
        sh.healthiness_checks_failed = Some(0);
        assert!(!sh.has_some_failed_healthchecks());

        sh.healthiness_checks_failed = Some(3);
        assert!(sh.has_some_failed_healthchecks());
    }

    #[test]
    fn test_restart_attempts_are_over() {
        // attempts=0 (default) → always "over" (first condition: attempts == 0)
        let sh: ServiceHandler = Service::from_name("svc").into();
        assert!(sh.restart_attempts_are_over());

        // attempts=5, restart_attempts=3 → not over
        let svc: Service =
            Service::from_str("command = \"test\"\n[restart]\nattempts = 5").unwrap();
        let mut sh: ServiceHandler = svc.into();
        sh.restart_attempts = 3;
        assert!(!sh.restart_attempts_are_over());

        // attempts=5, restart_attempts=6 → over
        sh.restart_attempts = 6;
        assert!(sh.restart_attempts_are_over());
    }

    // ========================================================================
    // should_force_kill edge cases
    // ========================================================================

    #[test]
    fn test_should_force_kill_forceful_shutdown() {
        let service: Service = toml::from_str(r#"command="x""#).unwrap();
        let mut sh: ServiceHandler = service.into();
        sh.pid = Some(Pid::this());
        sh.status = ServiceStatus::InKilling;
        // Forceful → always force kill (if has PID)
        assert!(should_force_kill(&sh, ShuttingDown::Forcefully));
        // No PID → never force kill, even if forceful
        sh.pid = None;
        assert!(!should_force_kill(&sh, ShuttingDown::Forcefully));
    }

    // ========================================================================
    // FSM event generation tests — next_events (normal operation)
    // ========================================================================

    use crate::horust::formats::RestartStrategy;
    use crate::horust::supervisor::LifecycleStatus;

    #[test]
    fn test_next_initial_runnable_emits_run() {
        let repo = make_repo(vec![("svc", ServiceStatus::Initial)]);
        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert_eq!(events, vec![Event::Run("svc".into())]);
    }

    #[test]
    fn test_next_initial_deps_not_met_emits_nothing() {
        let repo = make_repo_with_start_after(vec![
            ("dep", ServiceStatus::Initial, vec![]),
            ("svc", ServiceStatus::Initial, vec!["dep"]),
        ]);
        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert!(events.is_empty());
    }

    #[test]
    fn test_next_initial_deps_met_emits_run() {
        let repo = make_repo_with_start_after(vec![
            ("dep", ServiceStatus::Running, vec![]),
            ("svc", ServiceStatus::Initial, vec!["dep"]),
        ]);
        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert_eq!(events, vec![Event::Run("svc".into())]);
    }

    #[test]
    fn test_next_started_no_failed_healthchecks_becomes_running() {
        let mut repo = make_repo(vec![("svc", ServiceStatus::Started)]);
        {
            let sh = repo.services.get_mut("svc").unwrap();
            sh.healthiness_checks_failed = Some(0);
        }
        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert_eq!(
            events,
            vec![Event::new_status_update("svc", ServiceStatus::Running)]
        );
    }

    #[test]
    fn test_next_started_with_failed_healthchecks_stays() {
        // has_some_failed_healthchecks() returns true → no transition
        let repo = make_repo(vec![("svc", ServiceStatus::Started)]);
        // Default healthiness_checks_failed is None → unwrap_or(1) > 0 → true
        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert!(events.is_empty());
    }

    #[test]
    fn test_next_running_healthchecks_exceeded_kills() {
        let mut repo = make_repo(vec![("svc", ServiceStatus::Running)]);
        {
            let sh = repo.services.get_mut("svc").unwrap();
            sh.healthiness_checks_failed = Some(4); // > default max_failed of 3
        }
        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert_eq!(
            events,
            vec![
                Event::new_status_update("svc", ServiceStatus::InKilling),
                Event::Kill("svc".into()),
            ]
        );
    }

    #[test]
    fn test_next_running_healthchecks_ok_emits_nothing() {
        let mut repo = make_repo(vec![("svc", ServiceStatus::Running)]);
        {
            let sh = repo.services.get_mut("svc").unwrap();
            sh.healthiness_checks_failed = Some(2); // <= default max_failed of 3
        }
        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert!(events.is_empty());
    }

    #[test]
    fn test_next_success_with_never_restart_becomes_finished() {
        let repo = make_repo(vec![("svc", ServiceStatus::Success)]);
        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert_eq!(
            events,
            vec![Event::new_status_update("svc", ServiceStatus::Finished)]
        );
    }

    #[test]
    fn test_next_success_with_always_restart_becomes_initial() {
        let mut svc = Service::from_name("svc");
        svc.restart.strategy = RestartStrategy::Always;
        let mut repo = make_repo_from_services(vec![svc]);
        repo.services.get_mut("svc").unwrap().status = ServiceStatus::Success;

        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert_eq!(
            events,
            vec![Event::new_status_update("svc", ServiceStatus::Initial)]
        );
    }

    #[test]
    fn test_next_failed_with_ignore_strategy() {
        let mut svc = Service::from_name("svc");
        svc.failure.strategy = FailureStrategy::Ignore;
        svc.restart.strategy = RestartStrategy::OnFailure;
        let mut repo = make_repo_from_services(vec![svc]);
        repo.services.get_mut("svc").unwrap().status = ServiceStatus::Failed;

        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert!(events.contains(&Event::new_status_update("svc", ServiceStatus::Initial)));
    }

    #[test]
    fn test_next_failed_shutdown_strategy() {
        let mut svc = Service::from_name("svc");
        svc.failure.strategy = FailureStrategy::Shutdown;
        let mut repo = make_repo_from_services(vec![svc]);
        repo.services.get_mut("svc").unwrap().status = ServiceStatus::Failed;

        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        assert!(events.contains(&Event::ShuttingDownInitiated(ShuttingDown::Gracefully)));
    }

    #[test]
    fn test_next_failed_kill_dependents_strategy() {
        let mut svc = Service::from_name("svc");
        svc.failure.strategy = FailureStrategy::KillDependents;

        let mut dep = Service::from_name("dep");
        dep.start_after = vec!["svc".to_string()];

        let mut repo = make_repo_from_services(vec![svc, dep]);
        repo.services.get_mut("svc").unwrap().status = ServiceStatus::Failed;
        repo.services.get_mut("dep").unwrap().status = ServiceStatus::Running;

        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);

        assert!(events.contains(&Event::new_status_update("dep", ServiceStatus::InKilling)));
        assert!(events.contains(&Event::Kill("dep".into())));
    }

    #[test]
    fn test_next_in_killing_no_force_kill_emits_nothing() {
        let mut repo = make_repo(vec![("svc", ServiceStatus::InKilling)]);
        {
            let sh = repo.services.get_mut("svc").unwrap();
            sh.pid = Some(Pid::this());
            sh.shutting_down_started();
        }
        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, LifecycleStatus::Running);
        // Recently started shutting down, wait time not exceeded
        assert!(events.is_empty());
    }

    // ========================================================================
    // FSM event generation tests — shutdown
    // ========================================================================

    #[test]
    fn test_next_shutdown_state_transitions() {
        let graceful = LifecycleStatus::ShuttingDown(ShuttingDown::Gracefully);

        // Running/Started → InKilling + Kill
        for status in [ServiceStatus::Running, ServiceStatus::Started] {
            let repo = make_repo(vec![("svc", status.clone())]);
            let sh = repo.services.get("svc").unwrap();
            let events = sh.next(&repo, graceful);
            assert_eq!(
                events,
                vec![
                    Event::new_status_update("svc", ServiceStatus::InKilling),
                    Event::Kill("svc".into()),
                ],
                "Shutdown from {:?} should produce InKilling+Kill",
                status
            );
        }

        // Success/Initial → Finished
        for status in [ServiceStatus::Success, ServiceStatus::Initial] {
            let repo = make_repo(vec![("svc", status.clone())]);
            let sh = repo.services.get("svc").unwrap();
            let events = sh.next(&repo, graceful);
            assert_eq!(
                events,
                vec![Event::new_status_update("svc", ServiceStatus::Finished)],
                "Shutdown from {:?} should produce Finished",
                status
            );
        }

        // Failed → FinishedFailed
        let repo = make_repo(vec![("svc", ServiceStatus::Failed)]);
        let sh = repo.services.get("svc").unwrap();
        let events = sh.next(&repo, graceful);
        assert_eq!(
            events,
            vec![Event::new_status_update(
                "svc",
                ServiceStatus::FinishedFailed
            )]
        );

        // Already Finished/FinishedFailed → nothing
        for status in [ServiceStatus::Finished, ServiceStatus::FinishedFailed] {
            let repo = make_repo(vec![("svc", status)]);
            let sh = repo.services.get("svc").unwrap();
            assert!(sh.next(&repo, graceful).is_empty());
        }
    }

    #[test]
    fn test_next_shutdown_force_kill() {
        // InKilling + graceful + recently started → no force kill yet
        let mut repo = make_repo(vec![("svc", ServiceStatus::InKilling)]);
        {
            let sh = repo.services.get_mut("svc").unwrap();
            sh.pid = Some(Pid::this());
            sh.shutting_down_started();
        }
        let sh = repo.services.get("svc").unwrap();
        assert!(
            sh.next(
                &repo,
                LifecycleStatus::ShuttingDown(ShuttingDown::Gracefully)
            )
            .is_empty()
        );

        // InKilling + forceful → force kill immediately
        let mut repo = make_repo(vec![("svc", ServiceStatus::InKilling)]);
        repo.services.get_mut("svc").unwrap().pid = Some(Pid::this());
        let sh = repo.services.get("svc").unwrap();
        assert_eq!(
            sh.next(
                &repo,
                LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully)
            ),
            vec![Event::new_force_kill("svc")]
        );
    }
}
