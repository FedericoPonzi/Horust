use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, Healthiness, Service, ServiceName, ServiceStatus};
use std::collections::HashMap;
use std::time::Duration;

mod checks;
use checks::*;

// TODO:
// * Tunable healthchecks timing in horust's config
// * If there are no checks to run, just exit the thread. or go sleep until an "service created" event is received.
pub fn spawn(bus: BusConnector<Event>, services: Vec<Service>) {
    std::thread::spawn(move || {
        run(bus, services);
    });
}

#[derive(Debug)]
struct Repo {
    bus: BusConnector<Event>,

    services: HashMap<ServiceName, Service>,
    /// Keep track of services which have a pid and need check for progressing to the running state
    //TODO: just an hashset of servicenames
    started: HashMap<ServiceName, Service>,
    /// Keep track of running services
    running: HashMap<ServiceName, Service>,
    is_shutting_down: bool,
}

impl Repo {
    fn apply(&mut self, ev: Event) {
        debug!("received ev: {:?}", ev);
        if let Event::StatusChanged(service_name, new_status) = ev {
            let svc = self.services.get(&service_name).unwrap();
            if new_status == ServiceStatus::Started {
                self.started.insert(svc.name.clone(), svc.clone());
            } else if new_status == ServiceStatus::Running {
                let svc = self.started.remove(&service_name);
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
                self.started.remove(&service_name);
                self.running.remove(&service_name);
            }
        } else if ev == Event::ShuttingDownInitiated {
            self.is_shutting_down = true;
        } else if let Event::ForceKill(service_name) = ev {
            // If a service is killed, we don't need to check anymore
            self.started.remove(&service_name);
            self.running.remove(&service_name);
        }
    }

    fn ingest(&mut self) {
        self.bus
            .try_get_events()
            .into_iter()
            .for_each(|ev| self.apply(ev))
    }
    fn new(bus: BusConnector<Event>, services: Vec<Service>) -> Self {
        Self {
            bus,
            services: services
                .into_iter()
                .map(|service| (service.name.clone(), service))
                .collect(),
            started: Default::default(),
            running: Default::default(),
            is_shutting_down: false,
        }
    }

    fn send_ev(&mut self, ev: Event) {
        self.bus.send_event(ev)
    }
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
            check_health(&service.healthiness)
        })
        .map(|(s_name, _service)| Event::new_status_changed(s_name, ServiceStatus::Running));

    running
        .iter()
        .filter(|(_s_name, service)| !check_health(&service.healthiness))
        .map(|(service_name, _service)| {
            // TODO: change to ToBeKilled. If the healthcheck fails, maybe it's a transient failure and process might be still running.
            Event::Kill(service_name.into())
        })
        .chain(evs_starting)
        .collect()
}
fn run(bus: BusConnector<Event>, services: Vec<Service>) {
    let mut repo = Repo::new(bus, services);
    loop {
        repo.ingest();
        let events = next(&repo.started, &repo.running);
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
    use crate::horust::formats::{Event, Healthiness, Service, ServiceName, ServiceStatus};
    use crate::horust::healthcheck;
    use crate::horust::healthcheck::check_health;
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
        debug!("{:?}", events);
        assert!(events.contains(&Event::StatusChanged(
            service.name.clone(),
            ServiceStatus::Running
        )));
        Ok(())
    }
    #[test]
    fn test_healthiness_check_file() -> Result<()> {
        // _no_checks_needed
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
}
