use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, ExitStatus, Healthiness, Service, ServiceName, ServiceStatus};
#[cfg(feature = "http-healthcheck")]
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::time::Duration;

// TODO:
// * Tunable healthchecks timing in horust's config
// * If there are no checks to run, just exit the thread. or go sleep until an "service created" event is received.
pub fn spawn(bus: BusConnector, services: Vec<Service>) {
    std::thread::spawn(move || {
        run(bus, services);
    });
}

//TODO: we don't really need "service" here, but just the Healthiness section.
#[derive(Debug)]
struct Repo {
    bus: BusConnector,
    services: HashMap<ServiceName, Service>,
    /// Keep track of services which have a pid and need check for progressing to the running state
    starting: HashMap<ServiceName, Service>,
    /// Keep track of running services
    running: HashMap<ServiceName, Service>,
    is_shutting_down: bool,
}

impl Repo {
    fn apply(&mut self, ev: Event) {
        debug!("received ev: {:?}", ev);
        if let Event::StatusChanged(service_name, new_status) = ev {
            let svc = self.services.get(&service_name).unwrap();
            if new_status == ServiceStatus::Starting {
                self.starting.insert(svc.name.clone(), svc.clone());
            } else if new_status == ServiceStatus::Running {
                let svc = self.starting.remove(&service_name);
                self.running.insert(service_name, svc.unwrap());
            } else if vec![
                ServiceStatus::Finished,
                ServiceStatus::FinishedFailed,
                ServiceStatus::Failed,
                ServiceStatus::Finished,
            ]
            .contains(&new_status)
            {
                // If a service is finished, we don't need to check anymore
                self.starting.remove(&service_name);
                self.running.remove(&service_name);
            }
        } else if ev == Event::ShuttingDownInitiated {
            self.is_shutting_down = true;
        } else if let Event::ForceKill(service_name) = ev {
            // If a service is killed, we don't need to check anymore
            self.starting.remove(&service_name);
            self.running.remove(&service_name);
        }
    }

    fn ingest(&mut self) {
        self.bus
            .try_get_events()
            .into_iter()
            .for_each(|ev| self.apply(ev))
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
            is_shutting_down: false,
        }
    }

    fn send_ev(&mut self, ev: Event) {
        self.bus.send_event(ev)
    }
}

#[cfg(feature = "http-healthcheck")]
fn check_http_endpoint(endpoint: &str) -> bool {
    let client = Client::new();
    let resp: reqwest::blocking::Response = client.head(endpoint).send().unwrap();
    resp.status().is_success()
}

/// Returns true if the service is healthy and all checks are passed.
fn healthchecker(healthiness: &Healthiness) -> bool {
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
    checks == checks_res
}

/// Run the healthchecks, produce the event changes
fn next(
    running: &HashMap<ServiceName, Service>,
    starting: &HashMap<ServiceName, Service>,
) -> Vec<Event> {
    debug!("next");
    // Starting services don't go in failure state if they don't pass the healthcheck
    // TODO: probably add a timeout or trials.
    let evs_starting = starting
        .iter()
        .filter(|(s_name, service)| {
            debug!("going to check {}, which is starting...", s_name);
            healthchecker(&service.healthiness)
        })
        .map(|(s_name, _service)| Event::new_status_changed(s_name, ServiceStatus::Running));

    running
        .iter()
        .filter(|(_s_name, service)| !healthchecker(&service.healthiness))
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
        if repo.is_shutting_down {
            debug!("Breaking the loop..");
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    repo.send_ev(Event::Exiting("Healthcheck".into(), ExitStatus::Successful));
}

/// Setup require for the service, before running the healthchecks and starting the service.
pub fn prepare_service(healthiness: &Healthiness) -> Result<(), std::io::Error> {
    if let Some(file_path) = healthiness.file_path.as_ref() {
        std::fs::remove_file(file_path)?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::horust::error::Result;
    use crate::horust::formats::{Event, Healthiness, Service, ServiceName, ServiceStatus};
    use crate::horust::healthcheck;
    use crate::horust::healthcheck::healthchecker;
    use std::collections::HashMap;
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
        assert!(!healthchecker(&healthiness));
        std::fs::write(file_path, "Hello world!")?;
        assert!(healthchecker(&healthiness));
        let healthiness: Healthiness = Default::default();
        assert!(healthchecker(&healthiness));
        Ok(())
    }
}
