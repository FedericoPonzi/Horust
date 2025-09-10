//! If a service has defined an healthchecker, this module will spawn a worker to making sure that
//! the service is working as supposed to.

use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam::channel::{unbounded, Receiver, RecvTimeoutError, Sender};

use checks::*;

use crate::horust::bus::BusConnector;
use crate::horust::formats::{
    Event, Healthiness, HealthinessStatus, Service, ServiceName, ServiceStatus,
};

mod checks;

struct Worker {
    service: Service,
    bus: BusConnector<Event>,
    work_done_notifier: Receiver<()>,
}

impl Worker {
    fn new(service: Service, bus: BusConnector<Event>, work_done_notifier: Receiver<()>) -> Self {
        Worker {
            service,
            bus,
            work_done_notifier,
        }
    }
    pub fn spawn_thread(self) -> JoinHandle<()> {
        thread::spawn(move || self.run())
    }
    fn run(self) {
        loop {
            let status = check_health(&self.service.healthiness);
            self.bus.send_event(Event::HealthCheck(
                self.service.name.clone(),
                status.clone(),
            ));
            match self
                .work_done_notifier
                .recv_timeout(Duration::from_millis(1000))
            {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                _ => (),
            };
        }
    }
}

// TODO:
// * Tunable healthchecks timing in horust's config
// * If there are no checks to run, just exit the thread. or go sleep until an "service created" event is received.
pub fn spawn(bus: BusConnector<Event>, services: Vec<Service>) {
    thread::spawn(move || {
        run(bus, services);
    });
}

/// Returns true if the service is healthy and all checks are passed.
fn check_health(healthiness: &Healthiness) -> HealthinessStatus {
    get_checks()
        .into_iter()
        .all(|check| check.run(healthiness))
        .into()
}

fn run(bus: BusConnector<Event>, services: Vec<Service>) {
    let mut workers = hashmap! {};
    let get_service = |s_name: &ServiceName| {
        services
            .iter()
            .filter(|sh| sh.name == *s_name)
            .take(1)
            .cloned()
            .collect::<Vec<Service>>()
            .remove(0)
    };

    for ev in bus.iter() {
        match ev {
            Event::StatusChanged(s_name, ServiceStatus::Started) => {
                let service = get_service(&s_name);
                if !service.healthiness.has_any_check_defined() {
                    bus.send_event(Event::HealthCheck(s_name, HealthinessStatus::Healthy));
                    continue;
                }
                if let Some((sender, handler)) = workers.remove(&s_name) {
                    stop_worker(sender, handler)
                }
                let (worker_notifier, work_done_rcv) = unbounded();
                let handle = Worker::new(service, bus.join_bus(), work_done_rcv).spawn_thread();
                workers.insert(s_name, (worker_notifier, handle));
            }
            Event::ServiceExited(s_name, _exit_code) => {
                if let Some((sender, handler)) = workers.remove(&s_name) {
                    stop_worker(sender, handler)
                } else {
                    warn!("Worker thread for {} not found.", s_name);
                }
            }
            Event::ShuttingDownInitiated(_) => {
                // Stop all the workers:
                for (ws, _wh) in workers.values() {
                    // TODO: handle these
                    ws.send(()).unwrap();
                }
                // Actually wait for them
                for (_s_name, (_ws, wh)) in workers {
                    wh.join().unwrap();
                }
                break;
            }
            _ => {}
        }
    }
}

fn stop_worker(sender: Sender<()>, handler: JoinHandle<()>) {
    if let Err(error) = sender.send(()) {
        error!(
            "Cannot send msg to sender - channel might be closed. Error: {:?}",
            error
        );
    }
    if let Err(error) = handler.join() {
        error!("Error joining thread: {:?}", error);
    }
}

/// Setup require for the service, before running the healthchecks and starting the service
pub fn prepare_service(healthiness: &Healthiness) -> Result<Vec<()>, std::io::Error> {
    get_checks()
        .iter()
        .map(|check| check.prepare(healthiness))
        .collect()
}

#[cfg(test)]
mod test {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use anyhow::Result;
    use tempfile::TempDir;

    use crate::horust::formats::{Healthiness, HealthinessStatus};
    use crate::horust::healthcheck::{check_health, prepare_service};

    #[test]
    fn test_healthiness_check_file() -> Result<()> {
        let tempdir = TempDir::with_prefix("health")?;
        let file_path = tempdir.path().join("file.txt");
        let healthiness = Healthiness {
            file_path: Some(file_path.clone()),
            http_endpoint: None,
            ..Default::default()
        };
        assert_ne!(check_health(&healthiness), HealthinessStatus::Healthy);
        std::fs::write(file_path, "Hello world!")?;
        assert_eq!(check_health(&healthiness), HealthinessStatus::Healthy);
        let healthiness: Healthiness = Default::default();
        assert_eq!(check_health(&healthiness), HealthinessStatus::Healthy);
        Ok(())
    }

    fn handle_request(listener: TcpListener) -> std::io::Result<()> {
        if let Some(stream) = listener.incoming().next() {
            info!("Received request");
            let mut buffer = [0; 512];
            let mut stream = stream?;
            stream.read(&mut buffer).unwrap();
            let response = b"HTTP/1.1 200 OK\r\n\r\n";
            stream.write(response).expect("Stream write");
        }
        Ok(())
    }

    #[test]
    fn test_healthiness_http() -> Result<()> {
        let healthiness = Healthiness {
            file_path: None,
            http_endpoint: Some("http://localhost:123/".into()),
            ..Default::default()
        };
        assert_ne!(check_health(&healthiness), HealthinessStatus::Healthy);
        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        let socket = SocketAddrV4::new(loopback, 0);
        let listener = TcpListener::bind(socket)?;
        let port = listener.local_addr()?.port();
        let endpoint = format!("http://localhost:{port}");
        let healthiness = Healthiness {
            file_path: None,
            http_endpoint: Some(endpoint),
            ..Default::default()
        };
        let (sender, receiver) = mpsc::sync_channel(0);
        thread::spawn(move || {
            handle_request(listener).unwrap();
            sender.send(()).expect("Chan closed");
        });
        assert_eq!(check_health(&healthiness), HealthinessStatus::Healthy);
        receiver
            .recv_timeout(Duration::from_millis(2000))
            .expect("Failed to received response from handle_request");
        assert_ne!(check_health(&healthiness), HealthinessStatus::Healthy);
        Ok(())
    }

    #[test]
    fn test_healthiness_command() -> Result<()> {
        let tempdir = TempDir::with_prefix("health")?;
        let file_path = tempdir.path().join("file.txt");
        let healthiness = Healthiness {
            file_path: None,
            http_endpoint: None,
            command: Some(format!("cat {}", file_path.to_str().unwrap())),
            ..Default::default()
        };
        prepare_service(&healthiness)?;
        assert_ne!(check_health(&healthiness), HealthinessStatus::Healthy);
        std::fs::write(&file_path, "Hello world!")?;
        assert_eq!(check_health(&healthiness), HealthinessStatus::Healthy);
        let healthiness: Healthiness = Default::default();
        assert_eq!(check_health(&healthiness), HealthinessStatus::Healthy);
        Ok(())
    }
}
