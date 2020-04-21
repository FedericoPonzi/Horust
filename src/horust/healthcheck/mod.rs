use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, Healthiness, Service, ServiceName, ServiceStatus};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

mod checks;
mod repo;
use checks::*;
use repo::Repo;

// TODO:
// * Tunable healthchecks timing in horust's config
// * If there are no checks to run, just exit the thread. or go sleep until an "service created" event is received.
pub fn spawn(bus: BusConnector<Event>, services: Vec<Service>) {
    std::thread::spawn(move || {
        run(bus, services);
    });
}

/// Returns true if the service is healthy and all checks are passed.
fn check_health(healthiness: &Healthiness) -> bool {
    let file = FilePathCheck {};
    let http = HttpCheck {};
    let checks: Vec<&dyn Check> = vec![&file, &http];
    checks
        .into_iter()
        .filter(|check| !check.run(healthiness))
        .count()
        == 0
}

/// Run the healthchecks, produce the event changes
fn next(
    services: &HashMap<ServiceName, Service>,
    running: &HashSet<ServiceName>,
    starting: &HashSet<ServiceName>,
) -> Vec<Event> {
    debug!("next");
    // TODO: probably add a timeout or trials.
    starting
        .iter()
        .chain(running)
        .map(|s_name| (s_name, services.get(s_name).unwrap()))
        .filter_map(|(s_name, service)| {
            let has_passed_checks = check_health(&service.healthiness);
            if has_passed_checks && starting.contains(s_name) {
                Some(Event::new_status_changed(s_name, ServiceStatus::Running))
            } else if !has_passed_checks && running.contains(s_name) {
                // TODO: change to ToBeKilled. If the healthcheck fails, maybe it's a transient failure and process might be still running.
                Some(Event::Kill(s_name.into()))
            } else {
                // Starting services don't go in failure state if they don't pass the healthcheck
                None
            }
        })
        .collect()
}
fn run(bus: BusConnector<Event>, services: Vec<Service>) {
    let mut repo = Repo::new(bus, services);
    loop {
        repo.ingest();
        let events = next(&repo.services, &repo.started, &repo.running);
        for ev in events {
            repo.send_ev(ev);
        }
        if repo.is_shutting_down {
            debug!("Breaking the loop..");
            break;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }

    repo.send_ev(Event::new_exit_success("Healthcheck"));
}

/// Setup require for the service, before running the healthchecks and starting the service.
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
    use crate::horust::formats::{Event, Healthiness, Service, ServiceStatus};
    use crate::horust::healthcheck;
    use crate::horust::healthcheck::check_health;
    use std::collections::HashSet;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use tempdir::TempDir;

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
    }

    #[test]
    fn test_healthiness_check_file() -> Result<()> {
        let tempdir = TempDir::new("health")?;
        let file_path = tempdir.path().join("file.txt");
        let healthiness = Healthiness {
            file_path: Some(file_path.clone()),
            http_endpoint: None,
        };
        assert!(!check_health(&healthiness));
        std::fs::write(file_path, "Hello world!")?;
        assert!(check_health(&healthiness));
        let healthiness: Healthiness = Default::default();
        assert!(check_health(&healthiness));
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
        assert!(!check_health(&healthiness));
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
        assert!(check_health(&healthiness));
        receiver
            .recv_timeout(Duration::from_millis(2000))
            .expect("Failed to received response from handle_request");
        assert!(!check_health(&healthiness));
        Ok(())
    }
}
