use anyhow::Result;
use std::os::unix::net::UnixListener;

use horust_commands_lib::{ClientHandler, CommandsHandlerTrait, HorustMsgServiceStatus};
use log::info;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;

struct MockCommandsHandler {
    unix_listener: UnixListener,
}
impl MockCommandsHandler {
    // full socket path (not the folder).
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            unix_listener: UnixListener::bind(socket_path).unwrap(),
        }
    }
}
impl CommandsHandlerTrait for MockCommandsHandler {
    fn get_unix_listener(&mut self) -> &mut UnixListener {
        &mut self.unix_listener
    }

    fn get_service_status(&self, service_name: &str) -> Result<HorustMsgServiceStatus> {
        Ok(match service_name {
            "Running" => HorustMsgServiceStatus::Running,
            "Started" => HorustMsgServiceStatus::Started,
            _ => unimplemented!(),
        })
    }

    fn start_service(&self, _service_name: &str) -> Result<()> {
        Ok(())
    }

    fn stop_service(&self, _service_name: &str) -> Result<()> {
        Ok(())
    }

    fn update_service_status(
        &self,
        service_name: &str,
        new_status: HorustMsgServiceStatus,
    ) -> Result<()> {
        match new_status {
            HorustMsgServiceStatus::Initial => self.start_service(service_name),
            HorustMsgServiceStatus::Inkilling => self.stop_service(service_name),
            _ => Ok(()),
        }
    }

    fn restart_service(&self, _service_name: &str) -> Result<()> {
        Ok(())
    }

    fn reload_services(&self) -> Result<Vec<String>> {
        Ok(vec!["new-service.toml".to_string()])
    }

    fn get_all_service_statuses(&self) -> Vec<(String, HorustMsgServiceStatus)> {
        vec![
            ("Running".to_string(), HorustMsgServiceStatus::Running),
            ("Started".to_string(), HorustMsgServiceStatus::Started),
        ]
    }
}
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}
#[test]
fn test_simple() -> Result<()> {
    info!("Starting");
    init();

    let socket_path: PathBuf = "/tmp/simple.sock".into();
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }
    let socket_path2 = socket_path.clone();
    let barrier_server = Arc::new(Barrier::new(2));
    let barrier_client = Arc::clone(&barrier_server);
    let s_handle = thread::spawn(move || {
        let mut uds = MockCommandsHandler::new(socket_path2);
        info!("uds created");
        barrier_server.wait();
        uds.accept().unwrap();
        uds.accept().unwrap();
    });

    let c_handle = thread::spawn(move || {
        barrier_client.wait();
        let client = ClientHandler::new_client(&socket_path).unwrap();
        client.client("Running".into()).unwrap();

        let client = ClientHandler::new_client(&socket_path).unwrap();
        client.client("Started".into()).unwrap();
    });
    s_handle.join().unwrap();
    c_handle.join().unwrap();
    Ok(())
}

#[test]
fn test_change_request() -> Result<()> {
    init();
    let socket_path: PathBuf = "/tmp/test_change.sock".into();
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }
    let socket_path2 = socket_path.clone();
    let barrier = Arc::new(Barrier::new(2));
    let barrier2 = Arc::clone(&barrier);

    let s_handle = thread::spawn(move || {
        let mut uds = MockCommandsHandler::new(socket_path2);
        barrier.wait();
        uds.accept().unwrap();
    });

    let c_handle = thread::spawn(move || {
        barrier2.wait();
        let mut client = ClientHandler::new_client(&socket_path).unwrap();
        let (name, accepted) = client
            .send_change_request("Running".into(), HorustMsgServiceStatus::Inkilling)
            .unwrap();
        assert_eq!(name, "Running");
        assert!(accepted);
    });
    s_handle.join().unwrap();
    c_handle.join().unwrap();
    Ok(())
}

#[test]
fn test_restart_request() -> Result<()> {
    init();
    let socket_path: PathBuf = "/tmp/test_restart.sock".into();
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }
    let socket_path2 = socket_path.clone();
    let barrier = Arc::new(Barrier::new(2));
    let barrier2 = Arc::clone(&barrier);

    let s_handle = thread::spawn(move || {
        let mut uds = MockCommandsHandler::new(socket_path2);
        barrier.wait();
        uds.accept().unwrap();
    });

    let c_handle = thread::spawn(move || {
        barrier2.wait();
        let mut client = ClientHandler::new_client(&socket_path).unwrap();
        let (name, accepted) = client.send_restart_request("Running".into()).unwrap();
        assert_eq!(name, "Running");
        assert!(accepted);
    });
    s_handle.join().unwrap();
    c_handle.join().unwrap();
    Ok(())
}

#[test]
fn test_reload_request() -> Result<()> {
    init();
    let socket_path: PathBuf = "/tmp/test_reload.sock".into();
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }
    let socket_path2 = socket_path.clone();
    let barrier = Arc::new(Barrier::new(2));
    let barrier2 = Arc::clone(&barrier);

    let s_handle = thread::spawn(move || {
        let mut uds = MockCommandsHandler::new(socket_path2);
        barrier.wait();
        uds.accept().unwrap();
    });

    let c_handle = thread::spawn(move || {
        barrier2.wait();
        let mut client = ClientHandler::new_client(&socket_path).unwrap();
        let (accepted, new_services) = client.send_reload_request().unwrap();
        assert!(accepted);
        assert_eq!(new_services, vec!["new-service.toml"]);
    });
    s_handle.join().unwrap();
    c_handle.join().unwrap();
    Ok(())
}

#[test]
fn test_all_status_request() -> Result<()> {
    init();
    let socket_path: PathBuf = "/tmp/test_all_status.sock".into();
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }
    let socket_path2 = socket_path.clone();
    let barrier = Arc::new(Barrier::new(2));
    let barrier2 = Arc::clone(&barrier);

    let s_handle = thread::spawn(move || {
        let mut uds = MockCommandsHandler::new(socket_path2);
        barrier.wait();
        uds.accept().unwrap();
    });

    let c_handle = thread::spawn(move || {
        barrier2.wait();
        let mut client = ClientHandler::new_client(&socket_path).unwrap();
        let statuses = client.send_all_status_request().unwrap();
        assert_eq!(statuses.len(), 2);
        assert!(
            statuses
                .iter()
                .any(|(n, s)| n == "Running" && *s == HorustMsgServiceStatus::Running)
        );
        assert!(
            statuses
                .iter()
                .any(|(n, s)| n == "Started" && *s == HorustMsgServiceStatus::Started)
        );
    });
    s_handle.join().unwrap();
    c_handle.join().unwrap();
    Ok(())
}
