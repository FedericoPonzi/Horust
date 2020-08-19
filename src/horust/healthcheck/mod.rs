//! If a service has defined an healthchecker, this module will spawn a worker to making sure that
//! the service is working as supposed to.

use crate::horust::bus::BusConnector;
use crate::horust::formats::{
    Event, Healthiness, HealthinessStatus, Service, ServiceName, ServiceStatus,
};
use crossbeam::channel::{unbounded, Receiver, RecvTimeoutError};
use std::time::Duration;

mod checks;
use checks::*;
use std::thread;
use std::thread::JoinHandle;

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
    std::thread::spawn(move || {
        run(bus, services);
    });
}

/// Returns true if the service is healthy and all checks are passed.
fn check_health(healthiness: &Healthiness) -> HealthinessStatus {
    let failed_checks = get_checks()
        .into_iter()
        .filter(|check| !check.run(healthiness))
        .count();
    let is_healthy = failed_checks == 0;
    is_healthy.into()
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
                let (worker_notifier, work_done_rcv) = unbounded();
                let service = get_service(&s_name);
                let w = Worker::new(service, bus.clone(), work_done_rcv);
                let handle = w.spawn_thread();
                workers.insert(s_name, (worker_notifier, handle));
            }
            Event::ServiceExited(s_name, _exit_code) => {
                if let Some((sender, handler)) = workers.remove(&s_name) {
                    if sender.send(()).is_err() {
                        error!("Cannot send msg to sender - channel closed.");
                    }
                    if let Err(error) = handler.join() {
                        error!("Error joining thread: {:?}", error);
                    }
                } else {
                    warn!("Worker thread for {} not found.", s_name);
                }
            }
            Event::ShuttingDownInitiated => {
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

/// Setup require for the service, before running the healthchecks and starting the service
pub fn prepare_service(healthiness: &Healthiness) -> Result<Vec<()>, std::io::Error> {
    get_checks()
        .into_iter()
        .map(|check| check.prepare(healthiness))
        .collect()
}

#[cfg(test)]
mod test {
    use crate::horust::error::Result;
    use crate::horust::formats::{Healthiness, HealthinessStatus};
    use crate::horust::healthcheck::check_health;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use tempdir::TempDir;

    fn check_health_w(healthiness: &Healthiness) -> bool {
        check_health(healthiness) == HealthinessStatus::Healthy
    }
    #[test]
    fn test_healthiness_check_file() -> Result<()> {
        let tempdir = TempDir::new("health")?;
        let file_path = tempdir.path().join("file.txt");
        let healthiness = Healthiness {
            file_path: Some(file_path.clone()),
            http_endpoint: None,
            ..Default::default()
        };
        assert!(!check_health_w(&healthiness));
        std::fs::write(file_path, "Hello world!")?;
        assert!(check_health_w(&healthiness));
        let healthiness: Healthiness = Default::default();
        assert!(check_health_w(&healthiness));
        Ok(())
    }
    fn handle_request(listener: TcpListener) -> std::io::Result<()> {
        for stream in listener.incoming() {
            println!("Received request");
            let mut buffer = [0; 512];
            let mut stream = stream?;
            stream.read(&mut buffer).unwrap();
            let response = b"HTTP/1.1 200 OK\r\n\r\n";
            stream.write(response).expect("Stream write");
            break;
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
        assert!(!check_health_w(&healthiness));
        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        let socket = SocketAddrV4::new(loopback, 0);
        let listener = TcpListener::bind(socket)?;
        let port = listener.local_addr()?.port();
        let endpoint = format!("http://localhost:{}", port);
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
        assert!(check_health_w(&healthiness));
        receiver
            .recv_timeout(Duration::from_millis(2000))
            .expect("Failed to received response from handle_request");
        assert!(!check_health_w(&healthiness));
        Ok(())
    }
}
