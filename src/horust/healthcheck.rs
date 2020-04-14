use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, Healthiness, Service, ServiceName, ServiceStatus};
#[cfg(feature = "http-healthcheck")]
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::time::Duration;

// TODO:
// * Tunable healthchecks in horust's config
// * If there are no checks to run, just exit the thread. or go sleep until an "service created" event is received.
pub fn spawn(bus: BusConnector, services: Vec<Service>) {
    std::thread::spawn(move || {
        run(bus, services);
    });
}

struct Repo {
    bus: BusConnector,
    services: HashMap<ServiceName, Service>,
    starting: HashMap<ServiceName, Service>,
    running: HashMap<ServiceName, Service>,
}

impl Repo {
    fn ingest(&mut self) {
        self.bus.try_get_events().into_iter().for_each(|ev| {
            if let Event::StatusChanged(service_name, status) = ev {
                let svc = self.services.get(&service_name).unwrap();
                if status == ServiceStatus::Starting {
                    self.starting.insert(svc.name.clone(), svc.clone());
                } else if status == ServiceStatus::Running {
                    let svc = self.starting.remove(&service_name);
                    self.running.insert(service_name, svc.unwrap());
                }
            }
        });
    }
    fn new(bus: BusConnector, services: Vec<Service>) -> Self {
        Self {
            bus,
            services: services
                .into_iter()
                .map(|service| (service.name.clone(), service))
                .collect(),
            starting: Default::default(),
            running: Default::default(),
        }
    }
    fn send_ev(&mut self, ev: Event) {
        self.bus.send_event(ev)
    }
}

#[cfg(feature = "http-healthcheck")]
fn check_http_endpoint(endpoint: &str) -> bool {
    let client = Client::new();
    match client.head(endpoint).send() {
        Ok(resp) => resp.status().is_success(),
        Err(e) => {
            println!("{}", e);
            false
        }
    }
}

fn healthchecks(healthiness: &Healthiness) -> bool {
    // Count of required checks:
    let mut checks = 0;
    // Count of passed checks:
    let mut checks_res = 0;
    if let Some(file_path) = healthiness.file_path.as_ref() {
        checks += 1;
        checks_res += if file_path.exists() {
            1
        } else {
            debug!("Healthcheck: File: {:?}, doesn't exists yet.", file_path);
            0
        };
    }
    if let Some(endpoint) = healthiness.http_endpoint.as_ref() {
        let check_feature = |endpoint: &String| {
            #[cfg(not(feature = "http-healthcheck"))]
            {
                error!("There is an http based healthcheck for {}, requesting: {} , but horust was built without the http-healthcheck feature (thus it will never pass these checks).", sh.name(), endpoint);
                return (1, 0);
            }
            #[cfg(feature = "http-healthcheck")]
            return (1, if check_http_endpoint(endpoint) { 1 } else { 0 });
        };
        let (check, res) = check_feature(endpoint);
        checks += check;
        checks_res += res
    }
    /*
        Edge case: [healthcheck] header section is defined, but then it's empty. This should pass.
    */
    let res = checks <= checks_res;
    let empty_section = healthiness.file_path.is_some() || healthiness.http_endpoint.is_some();
    res || !empty_section
}

// Run the healthcheck, produce the event changes
fn next(
    running: &HashMap<ServiceName, Service>,
    starting: &HashMap<ServiceName, Service>,
) -> Vec<Event> {
    let evs_starting = starting
        .iter()
        .filter(|(_s_name, service)| healthchecks(&service.healthiness))
        .map(|(s_name, _service)| Event::new_status_changed(s_name, ServiceStatus::Running));
    running
        .iter()
        .filter(|(_s_name, service)| !healthchecks(&service.healthiness))
        .map(|(service_name, _service)| {
            // TODO: change to ToBeKilled. If the healthcheck fails, maybe it's a transient failure and process might be still running.
            Event::new_status_changed(service_name, ServiceStatus::Failed)
        })
        .chain(evs_starting)
        .collect()
}
fn run(bus: BusConnector, services: Vec<Service>) {
    let mut repo = Repo::new(bus, services);
    loop {
        repo.ingest();
        let events = next(&repo.starting, &repo.running);
        for ev in events {
            repo.send_ev(ev);
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// Setup require for the service, before running the healthchecks and starting the service.
pub fn prepare_service(healthiness: &Healthiness) -> Result<(), std::io::Error> {
    if let Some(file_path) = healthiness.file_path.as_ref() {
        std::fs::remove_file(file_path)?;
    }
    Ok(())
}

/* fn client_mock() -> bool {
    loop {
        match TcpStream::connect("127.0.0.1:9999") {
            Ok(mut _stream) => {
                println!("Successfully connected to server in port 9999");
                return true;
            }
            Err(e) if e.kind() == ErrorKind::ConnectionRefused => {
                continue;
            }
            Err(e) => {
                println!("Failed to connect: {}", e);
                return false;
            }
        }
    }
}
 */

#[cfg(test)]
mod test {
    use crate::horust::error::Result;
    use crate::horust::formats::{Event, Healthiness, Service, ServiceName, ServiceStatus};
    use crate::horust::healthcheck;
    use crate::horust::healthcheck::healthchecks;
    use std::collections::HashMap;
    use std::io::prelude::*;
    use std::io::ErrorKind;
    use std::net::TcpStream;
    use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
    use std::thread;
    use tempdir::TempDir;
    fn start_server(socket: SocketAddrV4) {
        let listener = TcpListener::bind(socket).unwrap();
        listener
            .set_nonblocking(true)
            .expect("Cannot set to non-blocking");
        // accept connections and process them, spawning a new thread for each one
        println!("Server listening on port: {}", socket.port());
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    // connection succeeded
                    handle_test_connection(stream);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    continue;
                    /* connection failed */
                }
                Err(_) => {}
            }
        }
    }
    fn handle_test_connection(mut stream: TcpStream) {
        let mut buffer = [0; 512];
        stream.read(&mut buffer).unwrap();
        let response = "HTTP/1.1 200 OK\r\n\r\n";
        stream.write(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    }
    #[test]
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
        std::fs::write(file_path, "Hello world!")?;
        let starting: HashMap<ServiceName, Service> = vec![(service.name.clone(), service.clone())]
            .into_iter()
            .collect();
        let events: Vec<Event> = healthcheck::next(&HashMap::new(), &starting);
        println!("{:?}", events);
        assert!(events.contains(&Event::StatusChanged(
            service.name.clone(),
            ServiceStatus::Running
        )));
        Ok(())
    }
    #[test]
    fn test_healthiness_checks() -> Result<()> {
        // _no_checks_needed
        let tempdir = TempDir::new("health")?;
        let file_path = tempdir.path().join("file.txt");
        let healthiness = Healthiness {
            file_path: Some(file_path.clone()),
            http_endpoint: None,
        };
        assert!(!healthchecks(&healthiness));
        std::fs::write(file_path, "Hello world!")?;
        assert!(healthchecks(&healthiness));
        let healthiness: Healthiness = Default::default();
        assert!(healthchecks(&healthiness));
        Ok(())
    }

    #[test]
    fn test_http_healthiness_check() {
        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        let socket = SocketAddrV4::new(loopback, 22222);
        let listener = TcpListener::bind(socket).unwrap();
        let local_addr = listener.local_addr().unwrap();
        let endpoint = format!("http://127.0.0.1:{}", local_addr.port());
        thread::spawn(move || {
            start_server(socket);
        });
        let healthiness = Healthiness {
            file_path: None,
            http_endpoint: Some(endpoint),
        };
        assert!(!healthchecks(&healthiness));
    }
}
