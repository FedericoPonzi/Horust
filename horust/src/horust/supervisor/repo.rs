use std::collections::HashMap;
use std::path::Path;

use nix::unistd::Pid;
use notify::event::ModifyKind;
use notify::{EventKind, Watcher};

use crate::horust::bus::BusConnector;
use crate::horust::formats::{Service, ServiceName};
use crate::horust::supervisor::service_handler::ServiceHandler;
use crate::horust::Event;

#[derive(Debug)]
pub(crate) struct Repo {
    pub services: HashMap<ServiceName, ServiceHandler>,
    pub(crate) bus: BusConnector<Event>,
    pub(crate) pid_map: HashMap<Pid, ServiceName>,
    _watcher: notify::RecommendedWatcher,
}

struct ConfigWatcher {
    bus: BusConnector<Event>,
}

impl notify::EventHandler for ConfigWatcher {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        if let Ok(notify::Event {
            kind: EventKind::Modify(ModifyKind::Data(_)),
            paths,
            attrs: _,
        }) = event
        {
            paths
                .iter()
                .for_each(|path| self.bus.send_event(Event::ReloadConfig(path.clone())));
        }
    }
}

impl Repo {
    pub(crate) fn new(bus: BusConnector<Event>, services: Vec<Service>) -> Self {
        let config_watcher = ConfigWatcher {
            bus: bus.join_bus(),
        };
        let mut watcher = notify::recommended_watcher(config_watcher).unwrap();
        services.iter().for_each(|service| {
            if let Some(path) = service.config_file.as_ref() {
                _ = watcher.watch(path.as_path(), notify::RecursiveMode::NonRecursive);
            }
        });

        let services = services
            .iter()
            .map(|service| (service.name.clone(), service.clone().into()))
            .collect();

        Self {
            bus,
            services,
            pid_map: HashMap::new(),
            _watcher: watcher,
        }
    }

    pub fn get_service_by_path(&self, path: &Path) -> Option<ServiceName> {
        self.services
            .iter()
            .find(|(_, handler)| {
                handler
                    .service()
                    .config_file
                    .as_ref()
                    .is_some_and(|config_file| config_file == path.as_os_str())
            })
            .map(|(service_name, _)| service_name.to_owned())
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
