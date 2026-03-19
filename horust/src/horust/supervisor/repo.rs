use std::collections::HashMap;

use nix::unistd::Pid;

use crate::horust::Event;
use crate::horust::bus::BusConnector;
use crate::horust::formats::{Service, ServiceName};
use crate::horust::supervisor::service_handler::ServiceHandler;

#[derive(Debug)]
pub(crate) struct Repo {
    pub services: HashMap<ServiceName, ServiceHandler>,
    pub(crate) bus: BusConnector<Event>,
    pub(crate) pid_map: HashMap<Pid, ServiceName>,
}

impl Repo {
    pub(crate) fn new(bus: BusConnector<Event>, services: Vec<Service>) -> Self {
        let services = services
            .into_iter()
            .map(|service| (service.name.clone(), service.into()))
            .collect();
        Self {
            bus,
            services,
            pid_map: HashMap::new(),
        }
    }
    pub(crate) fn insert_sh_by_name(&mut self, name: ServiceName, sh: ServiceHandler) {
        self.services.insert(name, sh);
    }
    pub(crate) fn get_service_by_pid(&self, pid: Pid) -> Option<&ServiceName> {
        self.pid_map.get(&pid)
    }

    pub(crate) fn add_pid(&mut self, pid: Pid, service: ServiceName) {
        self.pid_map.insert(pid, service);
    }
    pub(crate) fn remove_pid(&mut self, pid: Pid) {
        self.pid_map.remove(&pid);
    }

    /// Non blocking
    pub(crate) fn get_events(&mut self) -> Vec<Event> {
        self.bus.try_get_events()
    }

    pub fn all_have_finished(&self) -> bool {
        //TODO: This can be improved. When a service is finished, it can be added in a list, or even
        // a number. Then this check can be reduced to `return self.services.len() == self.finished_services`

        self.services
            .iter()
            .all(|(_s_name, sh)| sh.is_finished() || sh.is_finished_failed())
    }

    /// Get a mutable reference to the Service Handler
    pub fn get_mut_sh(&mut self, service_name: &str) -> &mut ServiceHandler {
        self.services.get_mut(service_name).unwrap()
    }

    /// Get an immutable reference to the Service Handler
    pub fn get_sh(&mut self, service_name: &str) -> &ServiceHandler {
        self.services.get(service_name).unwrap()
    }

    /// Get all the services that have specified "start-after = [`service_name`]" in their config
    pub(crate) fn get_dependents(&self, service_name: &str) -> Vec<ServiceName> {
        self.services
            .iter()
            .filter(|(_s_name, sh)| sh.service().start_after.contains(&service_name.to_string()))
            .map(|(s_name, _sh)| s_name)
            .cloned()
            .collect()
    }

    /// Get all the services that have specified "die-if-failed = [`service_name`]" in their config
    pub(crate) fn get_die_if_failed(&self, service_name: &str) -> Vec<&ServiceName> {
        self.services
            .iter()
            .filter(|(_s_name, sh)| {
                sh.service()
                    .termination
                    .die_if_failed
                    .contains(&service_name.to_string())
            })
            .map(|(s_name, _sh)| s_name)
            .collect()
    }

    pub(crate) fn send_ev(&mut self, ev: Event) {
        self.bus.send_event(ev)
    }

    /// Checks if the service is runnable. So the current status is Initial, and
    /// all the start-after have started or finished.
    pub(crate) fn is_service_runnable(&self, sh: &ServiceHandler) -> bool {
        if !sh.is_initial() {
            return false;
        }
        let is_started = |service_name: &ServiceName| {
            let sh = self.services.get(service_name).unwrap();
            sh.is_running() || sh.is_finished()
        };
        sh.start_after().iter().all(is_started)
    }

    pub(crate) fn any_finished_failed(&self) -> bool {
        self.services
            .iter()
            .any(|(_s_name, sh)| sh.is_finished_failed())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::horust::formats::ServiceStatus;

    use crate::horust::supervisor::test_utils::{
        make_repo, make_repo_from_services, make_repo_with_start_after,
    };

    // ========================================================================
    // all_have_finished tests
    // ========================================================================

    #[test]
    fn test_all_have_finished_when_all_finished() {
        let repo = make_repo(vec![
            ("a", ServiceStatus::Finished),
            ("b", ServiceStatus::Finished),
        ]);
        assert!(repo.all_have_finished());
    }

    #[test]
    fn test_all_have_finished_with_finished_failed() {
        let repo = make_repo(vec![
            ("a", ServiceStatus::Finished),
            ("b", ServiceStatus::FinishedFailed),
        ]);
        assert!(repo.all_have_finished());
    }

    #[test]
    fn test_all_have_finished_false_when_running() {
        let repo = make_repo(vec![
            ("a", ServiceStatus::Finished),
            ("b", ServiceStatus::Running),
        ]);
        assert!(!repo.all_have_finished());
    }

    #[test]
    fn test_all_have_finished_false_when_initial() {
        let repo = make_repo(vec![("a", ServiceStatus::Initial)]);
        assert!(!repo.all_have_finished());
    }

    // ========================================================================
    // any_finished_failed tests
    // ========================================================================

    #[test]
    fn test_any_finished_failed_true() {
        let repo = make_repo(vec![
            ("a", ServiceStatus::Finished),
            ("b", ServiceStatus::FinishedFailed),
        ]);
        assert!(repo.any_finished_failed());
    }

    #[test]
    fn test_any_finished_failed_false() {
        let repo = make_repo(vec![
            ("a", ServiceStatus::Finished),
            ("b", ServiceStatus::Finished),
        ]);
        assert!(!repo.any_finished_failed());
    }

    // ========================================================================
    // is_service_runnable tests
    // ========================================================================

    #[test]
    fn test_is_runnable_no_dependencies() {
        let repo = make_repo(vec![("svc", ServiceStatus::Initial)]);
        let sh = repo.services.get("svc").unwrap();
        assert!(repo.is_service_runnable(sh));
    }

    #[test]
    fn test_is_runnable_not_initial_state() {
        let repo = make_repo(vec![("svc", ServiceStatus::Running)]);
        let sh = repo.services.get("svc").unwrap();
        assert!(!repo.is_service_runnable(sh));
    }

    #[test]
    fn test_is_runnable_dependency_running() {
        let repo = make_repo_with_start_after(vec![
            ("dep", ServiceStatus::Running, vec![]),
            ("svc", ServiceStatus::Initial, vec!["dep"]),
        ]);
        let sh = repo.services.get("svc").unwrap();
        assert!(repo.is_service_runnable(sh));
    }

    #[test]
    fn test_is_runnable_dependency_finished() {
        let repo = make_repo_with_start_after(vec![
            ("dep", ServiceStatus::Finished, vec![]),
            ("svc", ServiceStatus::Initial, vec!["dep"]),
        ]);
        let sh = repo.services.get("svc").unwrap();
        assert!(repo.is_service_runnable(sh));
    }

    #[test]
    fn test_is_not_runnable_dependency_not_started() {
        let repo = make_repo_with_start_after(vec![
            ("dep", ServiceStatus::Initial, vec![]),
            ("svc", ServiceStatus::Initial, vec!["dep"]),
        ]);
        let sh = repo.services.get("svc").unwrap();
        assert!(!repo.is_service_runnable(sh));
    }

    #[test]
    fn test_is_not_runnable_dependency_starting() {
        let repo = make_repo_with_start_after(vec![
            ("dep", ServiceStatus::Starting, vec![]),
            ("svc", ServiceStatus::Initial, vec!["dep"]),
        ]);
        let sh = repo.services.get("svc").unwrap();
        assert!(!repo.is_service_runnable(sh));
    }

    #[test]
    fn test_is_runnable_multiple_deps_all_ready() {
        let repo = make_repo_with_start_after(vec![
            ("dep1", ServiceStatus::Running, vec![]),
            ("dep2", ServiceStatus::Finished, vec![]),
            ("svc", ServiceStatus::Initial, vec!["dep1", "dep2"]),
        ]);
        let sh = repo.services.get("svc").unwrap();
        assert!(repo.is_service_runnable(sh));
    }

    #[test]
    fn test_is_not_runnable_multiple_deps_partial() {
        let repo = make_repo_with_start_after(vec![
            ("dep1", ServiceStatus::Running, vec![]),
            ("dep2", ServiceStatus::Initial, vec![]),
            ("svc", ServiceStatus::Initial, vec!["dep1", "dep2"]),
        ]);
        let sh = repo.services.get("svc").unwrap();
        assert!(!repo.is_service_runnable(sh));
    }

    // ========================================================================
    // get_dependents tests
    // ========================================================================

    #[test]
    fn test_get_dependents_none() {
        let repo = make_repo_with_start_after(vec![
            ("a", ServiceStatus::Running, vec![]),
            ("b", ServiceStatus::Initial, vec![]),
        ]);
        let deps = repo.get_dependents("a");
        assert!(deps.is_empty());
    }

    #[test]
    fn test_get_dependents_single() {
        let repo = make_repo_with_start_after(vec![
            ("a", ServiceStatus::Running, vec![]),
            ("b", ServiceStatus::Initial, vec!["a"]),
        ]);
        let deps = repo.get_dependents("a");
        assert_eq!(deps, vec!["b".to_string()]);
    }

    #[test]
    fn test_get_dependents_multiple() {
        let repo = make_repo_with_start_after(vec![
            ("a", ServiceStatus::Running, vec![]),
            ("b", ServiceStatus::Initial, vec!["a"]),
            ("c", ServiceStatus::Initial, vec!["a"]),
            ("d", ServiceStatus::Initial, vec![]),
        ]);
        let mut deps = repo.get_dependents("a");
        deps.sort();
        assert_eq!(deps, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn test_get_dependents_nonexistent_service() {
        let repo = make_repo(vec![("a", ServiceStatus::Running)]);
        let deps = repo.get_dependents("nonexistent");
        assert!(deps.is_empty());
    }

    // ========================================================================
    // get_die_if_failed tests
    // ========================================================================

    #[test]
    fn test_get_die_if_failed_none() {
        let repo = make_repo(vec![
            ("a", ServiceStatus::Running),
            ("b", ServiceStatus::Running),
        ]);
        let result = repo.get_die_if_failed("a");
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_die_if_failed_single() {
        let mut svc_b = Service::from_name("b");
        svc_b.termination.die_if_failed = vec!["a".to_string()];

        let repo = make_repo_from_services(vec![Service::from_name("a"), svc_b]);

        let result = repo.get_die_if_failed("a");
        assert_eq!(result.len(), 1);
        assert_eq!(*result[0], "b");
    }

    // ========================================================================
    // PID map tests
    // ========================================================================

    #[test]
    fn test_add_and_get_pid() {
        let mut repo = make_repo(vec![("svc", ServiceStatus::Running)]);
        let pid = nix::unistd::Pid::from_raw(12345);
        repo.add_pid(pid, "svc".to_string());
        assert_eq!(repo.get_service_by_pid(pid), Some(&"svc".to_string()));
    }

    #[test]
    fn test_get_pid_not_found() {
        let repo = make_repo(vec![("svc", ServiceStatus::Running)]);
        let pid = nix::unistd::Pid::from_raw(99999);
        assert_eq!(repo.get_service_by_pid(pid), None);
    }

    #[test]
    fn test_remove_pid() {
        let mut repo = make_repo(vec![("svc", ServiceStatus::Running)]);
        let pid = nix::unistd::Pid::from_raw(12345);
        repo.add_pid(pid, "svc".to_string());
        repo.remove_pid(pid);
        assert_eq!(repo.get_service_by_pid(pid), None);
    }
}
