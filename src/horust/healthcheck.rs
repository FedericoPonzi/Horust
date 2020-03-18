use crate::horust::bus::BusConnector;
use crate::horust::formats::{Event, Service, ServiceHandler, ServiceName, ServiceStatus};
#[cfg(feature = "http-healthcheck")]
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::time::Duration;

// TODO:
// * Tunable healthchecks in horust's config
// * If there are no checks to run, just exit the thread. or go sleep until an "service created" event is received.
pub fn spawn(bus: BusConnector, services: Vec<Service>) {
    std::thread::spawn(move || {
        let mut repo = Repo::new(bus, services);
        loop {
            run_checks(&mut repo);
            std::thread::sleep(Duration::from_millis(1000));
        }
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
    let resp: reqwest::blocking::Response = client.head(endpoint).send().unwrap();
    resp.status().is_success()
}
fn healthchecks(service: &Service) -> bool {
    match service.healthiness.as_ref() {
        Some(healthiness) => {
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
            let empty_section =
                healthiness.file_path.is_some() || healthiness.http_endpoint.is_some();
            res || !empty_section
        }
        None => true,
    }
}
fn run_checks(repo: &mut Repo) {
    repo.ingest();
    let evs_starting: Vec<Event> = repo
        .starting
        .iter()
        .filter(|(_s_name, service)| healthchecks(service))
        .map(|(s_name, _service)| Event::new_status_changed(s_name, ServiceStatus::Running))
        .collect();
    let evs_running: Vec<Event> = repo
        .running
        .iter()
        .filter(|(_s_name, service)| !healthchecks(service))
        .map(|(service_name, _service)| {
            Event::new_status_changed(service_name, ServiceStatus::Failed)
        })
        .collect();

    for ev in evs_starting.into_iter().chain(evs_running) {
        repo.send_ev(ev);
    }
}

/// Setup require for the service, before running the healthchecks and starting the service.
pub fn prepare_service(service_handler: &ServiceHandler) -> Result<(), std::io::Error> {
    if let Some(healthiness) = &service_handler.service().healthiness {
        if let Some(file_path) = &healthiness.file_path {
            std::fs::remove_file(file_path)?;
        }
    }
    Ok(())
}

/*
#[cfg(test)]
mod test {
    use crate::horust::formats::{Healthness, Service, ServiceStatus};
    use crate::horust::{get_sample_service, healthcheck};
    use std::sync::Arc;

    fn create_from_service(service: Service) -> ServiceRepository {
        let services: Vec<Service> = vec![service];
        let services: ServiceRepository = ServiceRepository::new(services, UpdatesQueue);
        services.iter_mut().for_each(|sh| {
            sh.set_status(ServiceStatus::Starting);
        });
        services
    }

    fn assert_status(services: &Services, status: ServiceStatus) {
        services
            .0
            .lock()
            .unwrap()
            .iter()
            .for_each(|sh| assert_eq!(*sh.status(), status));
    }

    #[test]
    fn test_healthiness_checks() {
        // _no_checks_needed
        let service = get_sample_service().parse().unwrap();
        let services = create_from_service(service);
        healthcheck::run_checks(&Arc::clone(&services));
        assert_status(&services, ServiceStatus::Running);
    }

    #[test]
    fn test_check_file_path() {
        let tempdir = tempdir::TempDir::new("horust").unwrap();
        let filepath = tempdir.path().join("up");
        let healthcheck = Healthness {
            http_endpoint: None,
            file_path: Some(filepath.clone()),
        };
        let mut service: Service = get_sample_service().parse().unwrap();
        service.healthiness = Some(healthcheck);
        let services = create_from_service(service);
        healthcheck::run_checks(&Arc::clone(&services));
        assert_status(&services, ServiceStatus::Starting);
        std::fs::write(filepath, "Hello world!").unwrap();
        healthcheck::run_checks(&Arc::clone(&services));
        assert_status(&services, ServiceStatus::Running);
    }
}
*/
