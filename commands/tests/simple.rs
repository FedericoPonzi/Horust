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

    fn update_service_status(
        &self,
        service_name: &str,
        new_status: HorustMsgServiceStatus,
    ) -> Result<()> {
        todo!()
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
