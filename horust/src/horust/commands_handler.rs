use crate::horust::Event;
use crate::horust::bus::BusConnector;
use crate::horust::formats::{Service, ServiceName, ServiceStatus, User};
use anyhow::{Result, anyhow, bail};
use horust_commands_lib::{CommandsHandlerTrait, HorustMsgServiceStatus, UdsConnectionHandler};
use std::collections::HashMap;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;
use std::{fs, thread};

pub fn spawn(
    bus: BusConnector<Event>,
    uds_path: PathBuf,
    services: Vec<(ServiceName, User)>,
    services_paths: Vec<PathBuf>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut commands_handler = CommandsHandler::new(bus, uds_path, services, services_paths);
        commands_handler.run();
    })
}

struct CommandsHandler {
    bus: BusConnector<Event>,
    services: HashMap<ServiceName, ServiceStatus>,
    service_users: HashMap<ServiceName, User>,
    uds_listener: UnixListener,
    uds_path: PathBuf,
    services_paths: Vec<PathBuf>,
    /// Peer UID of the current connection being handled (set during accept).
    current_peer_uid: Option<u32>,
}

impl CommandsHandler {
    fn new(
        bus: BusConnector<Event>,
        uds_path: PathBuf,
        services: Vec<(ServiceName, User)>,
        services_paths: Vec<PathBuf>,
    ) -> Self {
        let uds_listener = UnixListener::bind(&uds_path).unwrap();
        uds_listener.set_nonblocking(true).unwrap();
        let service_users: HashMap<ServiceName, User> = services
            .iter()
            .map(|(name, user)| (name.clone(), user.clone()))
            .collect();
        Self {
            bus,
            uds_path,
            uds_listener,
            services_paths,
            service_users,
            current_peer_uid: None,
            services: services
                .into_iter()
                .map(|(s, _)| (s, ServiceStatus::Initial))
                .collect(),
        }
    }
    fn run(&mut self) {
        loop {
            let evs = self.bus.try_get_events();
            for ev in evs {
                match ev {
                    Event::StatusChanged(name, status) => {
                        if let Some(k) = self.services.get_mut(&name) {
                            *k = status;
                        }
                    }
                    Event::ServiceAdded(ref service) => {
                        self.services
                            .insert(service.name.clone(), ServiceStatus::Initial);
                        self.service_users
                            .insert(service.name.clone(), service.user.clone());
                    }
                    Event::ShuttingDownInitiated(_) => {
                        fs::remove_file(&self.uds_path).unwrap();
                        return;
                    }
                    _ => {}
                }
            }
            self.accept().unwrap();
            thread::sleep(Duration::from_millis(300));
        }
    }

    fn load_services_from_paths(paths: &[PathBuf]) -> Result<Vec<Service>> {
        let mut services = Vec::new();
        for path in paths {
            if !path.exists() {
                continue;
            }
            let entries = if path.is_file() {
                vec![path.to_path_buf()]
            } else {
                fs::read_dir(path)?
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
                            && p.extension()
                                .and_then(|ext| ext.to_str())
                                .is_some_and(|ext| ext == "toml")
                    })
                    .collect()
            };
            for entry in entries {
                match Service::from_file(&entry) {
                    Ok(mut svc) => {
                        if svc.name.is_empty() {
                            svc.name = entry
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                        }
                        if svc.name.is_empty() {
                            error!("Skipping service with empty name from {:?}", entry);
                            continue;
                        }
                        services.push(svc);
                    }
                    Err(err) => {
                        error!("Failed to load service from {:?}: {}", entry, err);
                    }
                }
            }
        }
        Ok(services)
    }

    /// Check if the current peer has permission to manage a given service.
    /// Root (UID 0) can manage all services. Non-root can only manage services
    /// where the service's user matches their UID.
    fn check_permission(&self, service_name: &str) -> Result<()> {
        let peer_uid = match self.current_peer_uid {
            Some(uid) => uid,
            None => bail!("Permission denied: unable to verify peer credentials."),
        };
        if peer_uid == 0 {
            return Ok(()); // Root can do anything
        }
        if let Some(user) = self.service_users.get(service_name) {
            match user.try_get_raw_uid() {
                Some(service_uid) if service_uid == peer_uid => Ok(()),
                Some(service_uid) => bail!(
                    "Permission denied: UID {peer_uid} cannot manage service '{service_name}' (owned by UID {service_uid})."
                ),
                None => {
                    bail!("Permission denied: cannot resolve owner of service '{service_name}'.")
                }
            }
        } else {
            Ok(())
        }
    }

    /// Extract peer UID from a Unix socket using SO_PEERCRED.
    #[cfg(target_os = "linux")]
    fn get_peer_uid(stream: &std::os::unix::net::UnixStream) -> Option<u32> {
        use std::os::unix::io::AsRawFd;
        let fd = stream.as_raw_fd();
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let ret = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut libc::ucred as *mut libc::c_void,
                &mut len,
            )
        };
        if ret == 0 { Some(cred.uid) } else { None }
    }

    #[cfg(not(target_os = "linux"))]
    fn get_peer_uid(_stream: &std::os::unix::net::UnixStream) -> Option<u32> {
        None
    }
}

impl CommandsHandlerTrait for CommandsHandler {
    fn get_unix_listener(&mut self) -> &mut UnixListener {
        &mut self.uds_listener
    }
    fn accept(&mut self) -> Result<()> {
        use std::io::ErrorKind;
        match self.get_unix_listener().accept() {
            Ok((stream, _addr)) => {
                self.current_peer_uid = Self::get_peer_uid(&stream);
                let conn_handler = UdsConnectionHandler::new(stream);
                if let Err(err) = self.handle_connection(conn_handler) {
                    error!("Error handling connection: {}", err);
                }
                self.current_peer_uid = None;
            }
            Err(e) => {
                if !matches!(e.kind(), ErrorKind::WouldBlock) {
                    error!("Error accepting connection: {e} - you might need to restart Horust.");
                }
            }
        };
        Ok(())
    }
    fn get_service_status(&self, service_name: &str) -> anyhow::Result<HorustMsgServiceStatus> {
        self.services
            .get(service_name)
            .map(from_service_status)
            .ok_or_else(|| anyhow!("Error: service {service_name} not found."))
    }
    fn update_service_status(
        &self,
        service_name: &str,
        new_status: HorustMsgServiceStatus,
    ) -> Result<()> {
        if !self.services.contains_key(service_name) {
            bail!("Service {service_name} not found.");
        }
        self.check_permission(service_name)?;
        match new_status {
            HorustMsgServiceStatus::Initial => {
                self.bus.send_event(Event::StatusUpdate(
                    service_name.to_string(),
                    ServiceStatus::Initial,
                ));
                Ok(())
            }
            HorustMsgServiceStatus::Inkilling => {
                self.bus.send_event(Event::StatusUpdate(
                    service_name.to_string(),
                    ServiceStatus::InKilling,
                ));
                self.bus.send_event(Event::Kill(service_name.to_string()));
                Ok(())
            }
            _ => bail!("Only INITIAL (start) and INKILLING (stop) are supported change targets."),
        }
    }
    fn restart_service(&self, service_name: &str) -> Result<()> {
        if !self.services.contains_key(service_name) {
            bail!("Service {service_name} not found.");
        }
        self.check_permission(service_name)?;
        self.bus
            .send_event(Event::Restart(service_name.to_string()));
        Ok(())
    }
    fn reload_services(&self) -> Result<Vec<String>> {
        let all_on_disk = Self::load_services_from_paths(&self.services_paths)?;
        let new_services: Vec<Service> = all_on_disk
            .into_iter()
            .filter(|s| !self.services.contains_key(&s.name))
            .collect();
        let new_names: Vec<String> = new_services.iter().map(|s| s.name.clone()).collect();
        for service in new_services {
            self.bus.send_event(Event::ServiceAdded(service));
        }
        Ok(new_names)
    }
    fn get_all_service_statuses(&self) -> Vec<(String, HorustMsgServiceStatus)> {
        let mut statuses: Vec<(String, HorustMsgServiceStatus)> = self
            .services
            .iter()
            .map(|(name, status)| (name.clone(), from_service_status(status)))
            .collect();
        statuses.sort_by(|a, b| a.0.cmp(&b.0));
        statuses
    }
}

fn from_service_status(status: &ServiceStatus) -> HorustMsgServiceStatus {
    match status {
        ServiceStatus::Starting => HorustMsgServiceStatus::Starting,
        ServiceStatus::Started => HorustMsgServiceStatus::Started,
        ServiceStatus::Running => HorustMsgServiceStatus::Running,
        ServiceStatus::InKilling => HorustMsgServiceStatus::Inkilling,
        ServiceStatus::Success => HorustMsgServiceStatus::Success,
        ServiceStatus::Finished => HorustMsgServiceStatus::Finished,
        ServiceStatus::FinishedFailed => HorustMsgServiceStatus::Finishedfailed,
        ServiceStatus::Failed => HorustMsgServiceStatus::Failed,
        ServiceStatus::Initial => HorustMsgServiceStatus::Initial,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horust::bus::Bus;

    /// Build a minimal CommandsHandler for permission testing.
    /// Does not bind a real socket — only used for check_permission tests.
    fn make_handler_for_perms(
        test_name: &str,
        peer_uid: Option<u32>,
        services: Vec<(&str, User)>,
    ) -> CommandsHandler {
        let bus: Bus<Event> = Bus::new();
        let connector = bus.join_bus();
        std::thread::spawn(move || bus.run());

        let socket_path: PathBuf = format!("/tmp/__test_perm_{}.sock", test_name).into();
        let _ = std::fs::remove_file(&socket_path);
        let uds_listener = UnixListener::bind(&socket_path).unwrap();

        let service_users: HashMap<ServiceName, User> = services
            .iter()
            .map(|(name, user)| (name.to_string(), user.clone()))
            .collect();
        let services_map: HashMap<ServiceName, ServiceStatus> = services
            .iter()
            .map(|(name, _)| (name.to_string(), ServiceStatus::Running))
            .collect();

        let handler = CommandsHandler {
            bus: connector,
            services: services_map,
            service_users,
            uds_listener,
            uds_path: socket_path,
            services_paths: vec![],
            current_peer_uid: peer_uid,
        };
        handler
    }

    /// Regression: when current_peer_uid is None (SO_PEERCRED failed or non-Linux),
    /// check_permission must deny access (fail-closed), not allow it.
    #[test]
    fn test_permission_denies_when_peer_uid_unknown() {
        let handler = make_handler_for_perms(
            "unknown",
            None, // No peer UID available
            vec![("svc", User::Uid(1000))],
        );
        let result = handler.check_permission("svc");
        assert!(
            result.is_err(),
            "check_permission should deny when peer UID is unknown"
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied"),
            "Error message should mention permission denied"
        );
        let _ = std::fs::remove_file(&handler.uds_path);
    }

    /// Root (UID 0) should always be allowed.
    #[test]
    fn test_permission_allows_root() {
        let handler = make_handler_for_perms(
            "root",
            Some(0), // Root
            vec![("svc", User::Uid(1000))],
        );
        assert!(handler.check_permission("svc").is_ok());
        let _ = std::fs::remove_file(&handler.uds_path);
    }

    /// Non-root user matching the service's UID should be allowed.
    #[test]
    fn test_permission_allows_matching_uid() {
        let handler = make_handler_for_perms("match", Some(1000), vec![("svc", User::Uid(1000))]);
        assert!(handler.check_permission("svc").is_ok());
        let _ = std::fs::remove_file(&handler.uds_path);
    }

    /// Non-root user with mismatched UID should be denied.
    #[test]
    fn test_permission_denies_mismatched_uid() {
        let handler =
            make_handler_for_perms("mismatch", Some(1001), vec![("svc", User::Uid(1000))]);
        let result = handler.check_permission("svc");
        assert!(result.is_err());
        let _ = std::fs::remove_file(&handler.uds_path);
    }

    /// Regression: when User::Name refers to a non-existent username,
    /// try_get_raw_uid returns None and permission should be denied (fail-closed).
    #[test]
    fn test_permission_denies_unresolvable_username() {
        let handler = make_handler_for_perms(
            "unresolvable",
            Some(1000),
            vec![("svc", User::Name("nonexistent_user_xyz_12345".to_string()))],
        );
        let result = handler.check_permission("svc");
        assert!(
            result.is_err(),
            "check_permission should deny when service owner username cannot be resolved"
        );
        assert!(
            result.unwrap_err().to_string().contains("cannot resolve"),
            "Error should mention cannot resolve"
        );
        let _ = std::fs::remove_file(&handler.uds_path);
    }
}
