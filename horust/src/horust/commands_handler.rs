use crate::horust::bus::BusConnector;
use crate::horust::formats::{ServiceName, ServiceStatus};
use crate::horust::Event;
use anyhow::{anyhow, Result};
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
        let uds_listener = UnixListener::bind(&uds_path).unwrap();
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
    fn get_service_status(&self, service_name: &str) -> anyhow::Result<HorustMsgServiceStatus> {
        self.services
            .get(service_name)
            .map(from_service_status)
            .ok_or_else(|| anyhow!("Error: service {service_name} not found."))
    }
    fn update_service_status(
        &self,
        _service_name: &str,
        _new_status: HorustMsgServiceStatus,
    ) -> Result<()> {
        /*
        match self.services.get(service_name) {
            None => bail!("Service {service_name} not found."),
            Some(service_status) if from_service_status(service_status) != new_status => {
                //self.bus.send_event(Event::Kill())
            }
            _ => (),
        };*/
        todo!();
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
