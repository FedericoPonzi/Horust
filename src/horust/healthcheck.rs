use crate::horust::formats::ServiceStatus;
use crate::horust::service_handler::{ServiceHandler, ServiceRepository};

#[cfg(feature = "http-healthcheck")]
use reqwest::blocking::Client;
use std::time::Duration;

// TODO: this is not really healthiness check, but rather readiness check. please change.
// TODO: If the healthcheck fails and status wasn't initial, set to failed.
pub fn spawn(mut services: ServiceRepository) {
    std::thread::spawn(move || loop {
        run_checks(&mut services);
        std::thread::sleep(Duration::from_millis(200));
    });
}

#[cfg(feature = "http-healthcheck")]
fn check_http_endpoint(endpoint: &str) -> bool {
    let client = Client::new();
    let resp: reqwest::blocking::Response = client.head(endpoint).send().unwrap();
    resp.status().is_success()
}

fn run_checks(services: &mut ServiceRepository) {
    services.ingest("healthcheck");
    let healthchecks = |sh: &ServiceHandler| match sh.service().healthiness.as_ref() {
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
    };
    services.mutate_service_status(|sh| {
        if sh.is_starting() && healthchecks(sh) {
            sh.set_status(ServiceStatus::Running);
            Some(sh)
        } else {
            None
        }
    });
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

#[cfg(test)]
mod test {
    /*
    use crate::horust::formats::{Healthness, Service, ServiceStatus, UpdatesQueue};
    use crate::horust::service_handler::ServiceRepository;
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
    */
}
