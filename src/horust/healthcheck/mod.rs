use crate::horust::bus::BusConnector;
use crate::horust::formats::{
    Event, Healthiness, HealthinessStatus, Service, ServiceName, ServiceStatus,
};
use crossbeam::channel::{unbounded, Receiver, Sender};
use std::time::Duration;

mod checks;
use checks::*;
use std::thread;
use std::thread::JoinHandle;

impl From<bool> for HealthinessStatus {
    fn from(check: bool) -> Self {
        if check {
            HealthinessStatus::Healthy
        } else {
            HealthinessStatus::Unhealthy
        }
    }
}

struct Worker {
    service: Service,
    sender_res: Sender<Event>,
    work_done_notifier: Receiver<()>,
}
impl Worker {
    fn new(service: Service, sender_res: Sender<Event>, work_done_notifier: Receiver<()>) -> Self {
        Worker {
            service,
            sender_res,
            work_done_notifier,
        }
    }
    pub fn spawn_thread(self) -> JoinHandle<()> {
        thread::spawn(move || self.run())
    }
    fn run(self) {
        let mut last = HealthinessStatus::Unhealthy;
        loop {
            let status = check_health(&self.service.healthiness);
            if status != last {
                // TODO: healthy / unhealthy
                self.sender_res
                    .send(Event::HealthCheck(
                        self.service.name.clone(),
                        status.clone(),
                    ))
                    .unwrap();
                last = status;
            }
            let work_done = self
                .work_done_notifier
                .recv_timeout(Duration::from_millis(300));

            if work_done.is_ok() {
                break;
            }
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
    let file = FilePathCheck {};
    let http = HttpCheck {};
    let checks: Vec<&dyn Check> = vec![&file, &http];
    let res = checks
        .into_iter()
        .filter(|check| !check.run(healthiness))
        .count()
        == 0;
    res.into()
}

fn run(bus: BusConnector<Event>, services: Vec<Service>) {
    //let mut repo = Repo::new(bus, services);
    let (health_snd, health_rcv) = unbounded();
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
    'main: loop {
        for ev in bus.try_get_events() {
            if let Event::StatusChanged(s_name, status) = ev {
                match status {
                    ServiceStatus::Started => {
                        let (worker_notifier, work_done_rcv) = unbounded();
                        let service = get_service(&s_name);
                        let w = Worker::new(service, health_snd.clone(), work_done_rcv);
                        let handle = w.spawn_thread();
                        workers.insert(s_name, (worker_notifier, handle));
                    }
                    ServiceStatus::InKilling
                    | ServiceStatus::Finished
                    | ServiceStatus::FinishedFailed => {
                        if let Some((sender, handler)) = workers.remove(&s_name) {
                            sender.send(()).unwrap();
                            handler.join().unwrap();
                        }
                    }
                    _ => (),
                }
            } else if ev == Event::ShuttingDownInitiated {
                // Stop all the workers:
                for (ws, _wh) in workers.values() {
                    ws.send(()).unwrap();
                }
                // Actually wait for them
                for (_s_name, (_ws, wh)) in workers {
                    wh.join().unwrap();
                }
                break 'main;
            }
        }
        let events: Vec<Event> = health_rcv.try_iter().collect();
        for ev in events {
            bus.send_event(ev);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    bus.send_event(Event::new_exit_success("Healthchecker"));
}

/// Setup require for the service, before running the healthchecks and starting the service
pub fn prepare_service(healthiness: &Healthiness) -> Result<(), std::io::Error> {
    if let Some(file_path) = healthiness.file_path.as_ref() {
        //TODO: check if user has permissions to remove this file.
        std::fs::remove_file(file_path)?;
    }
    Ok(())
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

    /*#[test]
        fn test_next() -> Result<()> {
            let tempdir = TempDir::new("health")?;
            let file_path = tempdir.path().join("file.txt");
            let service = format!(
                r#"command = "not relevant"
    [healthiness]
    file-path = "{}""#,
                file_path.display()
            );
            let service: Service = toml::from_str(service.as_str())?;
            let services = hashmap! {service.name.clone() => service.clone()};
            std::fs::write(file_path, "Hello world!")?;
            let starting = hashset! {service.name.clone()};
            let events: Vec<Event> = healthcheck::next(&services, &HashSet::new(), &starting);
            debug!("{:?}", events);
            assert!(events.contains(&Event::StatusChanged(
                service.name.clone(),
                ServiceStatus::Running
            )));
            Ok(())
        }*/
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
