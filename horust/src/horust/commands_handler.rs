use crate::horust::bus::BusConnector;
use crate::horust::Event;
use horust_commands_lib::{CommandsHandlerTrait, HorustMsgServiceStatus};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread;
use std::thread::JoinHandle;

pub fn spawn(bus: BusConnector<Event>, uds_folder_path: PathBuf) -> JoinHandle<()> {
    thread::spawn(move || {
        run(bus, uds_folder_path);
    })
}

fn run(bus: BusConnector<Event>, uds_folder_path: PathBuf) {
    let mut commands_handler = CommandsHandler::new(bus, uds_folder_path);
    commands_handler.start();
}

struct CommandsHandler {
    bus: BusConnector<Event>,
    uds_listener: UnixListener,
}
impl CommandsHandler {
    fn new(bus: BusConnector<Event>, uds_folder_path: PathBuf) -> Self {
        Self {
            bus,
            uds_listener: UnixListener::bind(uds_folder_path).unwrap(),
        }
    }
}

impl CommandsHandlerTrait for CommandsHandler {
    fn get_unix_listener(&mut self) -> &mut UnixListener {
        &mut self.uds_listener
    }

    fn get_service_status(&self, service_name: String) -> HorustMsgServiceStatus {
        todo!()
    }
}
