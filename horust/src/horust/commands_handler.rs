use crate::horust::bus::BusConnector;
use crate::horust::formats::{ServiceName, ServiceStatus};
use crate::horust::Event;
use horust_commands_lib::{CommandsHandlerTrait, HorustMsgServiceStatus};
use std::collections::HashMap;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;
use std::{fs, thread};

pub fn spawn(
    bus: BusConnector<Event>,
    uds_path: PathBuf,
    services: Vec<ServiceName>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut commands_handler = CommandsHandler::new(bus, uds_path, services);
        commands_handler.run();
    })
}

struct CommandsHandler {
    bus: BusConnector<Event>,
    services: HashMap<ServiceName, ServiceStatus>,
    uds_listener: UnixListener,
    uds_path: PathBuf,
}

impl CommandsHandler {
    fn new(bus: BusConnector<Event>, uds_path: PathBuf, services: Vec<ServiceName>) -> Self {
        let mut uds_listener = UnixListener::bind(&uds_path).unwrap();
        uds_listener.set_nonblocking(true).unwrap();
        Self {
            bus,
            uds_path,
            uds_listener,
            services: services
                .into_iter()
                .map(|s| (s, ServiceStatus::Initial))
                .collect(),
        }
    }
    fn run(&mut self) {
        loop {
            let evs = self.bus.try_get_events();
            for ev in evs {
                match ev {
                    Event::StatusChanged(name, status) => {
                        let k = self.services.get_mut(&name).unwrap();
                        *k = status;
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
}

impl CommandsHandlerTrait for CommandsHandler {
    fn get_unix_listener(&mut self) -> &mut UnixListener {
        &mut self.uds_listener
    }

    fn get_service_status(&self, service_name: String) -> HorustMsgServiceStatus {
        todo!()
    }
}
