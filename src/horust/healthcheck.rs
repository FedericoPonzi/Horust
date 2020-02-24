use crate::horust::formats::ServiceStatus;
use crate::horust::service_handler::ServiceHandler;
use crate::horust::Services;
#[cfg(feature = "http-healthcheck")]
use reqwest::blocking::Client;

// TODO: this is not really healthiness check, but rather readiness check. please change.
pub(crate) fn healthcheck_entrypoint(services: Services) {
    loop {
        run_checks(&services)
    }
}
#[cfg(feature = "http-healthcheck")]
fn check_http_endpoint(endpoint: &str) -> bool {
    let client = Client::new();
    let resp: reqwest::blocking::Response = client.head(endpoint).send().unwrap();
    resp.status().is_success()
}

fn run_checks(services: &Services) {
    services
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|sh| sh.is_starting())
        .filter(|sh| match sh.service().healthiness.as_ref() {
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
                #[cfg(feature = "http-healthcheck")]
                {
                    if let Some(endpoint) = healthiness.http_endpoint.as_ref() {
                        checks += 1;
                        checks_res += if check_http_endpoint(endpoint) { 1 } else { 0 };
                    }
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
        })
        .for_each(|sh| sh.status = ServiceStatus::Running);
}

/// Setup require for the service, before running the healthchecks and starting the service.
pub(crate) fn prepare_service(service_handler: &ServiceHandler) -> Result<(), std::io::Error> {
    if let Some(healthiness) = &service_handler.service().healthiness {
        if let Some(file_path) = &healthiness.file_path {
            std::fs::remove_file(file_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::horust::formats::{Healthness, Service, ServiceStatus};
    use crate::horust::{get_sample_service, healthcheck, ServiceHandler, Services};
    use std::sync::{Arc, Mutex};
    fn create_from_service(service: Service) -> Services {
        let services: Vec<Service> = vec![service];
        let services: Services = Arc::new(Mutex::new(
            services.into_iter().map(ServiceHandler::from).collect(),
        ));
        services.lock().unwrap().iter_mut().for_each(|sh| {
            sh.set_status(ServiceStatus::Starting);
        });
        services
    }
    fn assert_status(services: &Services, status: ServiceStatus) {
        services
            .lock()
            .unwrap()
            .iter()
            .for_each(|sh| assert_eq!(sh.status, status));
    }
    #[test]
    fn test_healthiness_checks() {
        // _no_checks_needed
        let service = get_sample_service().parse().unwrap();
        let services = create_from_service(service);
        healthcheck::run_checks(&services);
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
        healthcheck::run_checks(&services);
        assert_status(&services, ServiceStatus::Starting);
        std::fs::write(filepath, "Hello world!").unwrap();
        healthcheck::run_checks(&services);
        assert_status(&services, ServiceStatus::Running);
    }
}
