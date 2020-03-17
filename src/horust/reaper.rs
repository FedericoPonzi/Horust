use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, ServiceName, ServiceStatus};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub(crate) fn spawn(bus: BusConnector) {
    std::thread::spawn(move || {
        supervisor_thread(bus);
    });
}

struct Repo {
    possibly_running: HashSet<ServiceName>,
    bus: BusConnector,
    pids_map: HashMap<Pid, ServiceName>,
}

impl Repo {
    fn new(bus: BusConnector) -> Self {
        Repo {
            possibly_running: HashSet::new(),
            pids_map: HashMap::new(),
            bus,
        }
    }

    fn consume(&mut self, ev: Event) {
        match ev {
            Event::PidChanged(service_name, pid) => {
                self.pids_map.insert(pid, service_name);
            }
            Event::StatusChanged(service_name, status) => {
                if vec![ServiceStatus::ToBeRun, ServiceStatus::Initial].contains(&status) {
                    self.possibly_running.insert(service_name);
                } else {
                    self.possibly_running.remove(&service_name);
                }
            }
            _ => (),
        }
    }
    fn send_pid_exited(&mut self, pid: Pid, exit_code: i32) {
        if self.pids_map.contains_key(&pid) {
            let service_name = self.pids_map.remove(&pid).unwrap();
            self.bus
                .send_event(Event::new_service_exited(service_name, exit_code));
        }
    }

    fn ingest(&mut self) {
        let updates: Vec<Event> = self.bus.try_get_events();
        updates.into_iter().for_each(|ev| self.consume(ev));
    }
}

/// A endlessly running function meant to be run in a separate thread.
/// Its purpose is to continuously try to reap possibly dead children.
pub(crate) fn supervisor_thread(bus: BusConnector) {
    let mut reapable = HashMap::new();
    let mut repo = Repo::new(bus);

    loop {
        repo.ingest();
        match waitpid(Pid::from_raw(-1), None) {
            Ok(wait_status) => {
                if let WaitStatus::Exited(pid, exit_code) = wait_status {
                    debug!("Pid has exited: {} with exitcode: {}", pid, exit_code);
                    reapable.insert(pid, exit_code);
                }
            }
            Err(err) => {
                if !err.to_string().contains("ECHILD") {
                    error!("Error waitpid(): {}", err);
                }
            }
        }
        // It might happen that before supervised was updated, the process was already started, executed,
        // and exited. Thus we're trying to reaping it, but there is still no map Pid -> Service.
        reapable.retain(|pid, exit_code| {
            if repo.pids_map.contains_key(pid) {
                repo.send_pid_exited(*pid, *exit_code);
                true
            } else {
                // If is a grandchildren, we don't care about it:
                // is grandchildren =
                repo.possibly_running.is_empty()
            }
        });
        std::thread::sleep(Duration::from_millis(500))
    }
}
