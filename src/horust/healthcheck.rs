use crate::horust::formats::{Healthness, ServiceStatus};
use crate::horust::{ServiceHandler, Services};
use reqwest::blocking::Client;

// TODO: this is not really healthness check, but rather readiness check. please change.
pub fn healthcheck_entrypoint(services: Services) {
    loop {
        run_checks(&services)
    }
}
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
        .filter(|sh| match sh.service.healthness.as_ref() {
            Some(healthness) => {
                // Count of required checks:
                let mut checks = 0;
                // Count of passed checks:
                let mut checks_res = 0;
                if let Some(file_path) = healthness.file_path.as_ref() {
                    checks += 1;
                    checks_res += if file_path.exists() {
                        1
                    } else {
                        debug!("Healthcheck: File: {:?}, doesn't exists yet.", file_path);
                        0
                    };
                }
                if let Some(endpoint) = healthness.http_endpoint.as_ref() {
                    checks += 1;
                    checks_res += if check_http_endpoint(endpoint) { 1 } else { 0 };
                }
                /*
                    Edge case: [healthcheck] header section is defined, but then it's empty. This should pass.
                */
                let res = checks == checks_res;
                let empty_section =
                    healthness.file_path.is_some() || healthness.http_endpoint.is_some();
                res || !empty_section
            }
            None => true,
        })
        .for_each(|sh| sh.status = ServiceStatus::Running);
}

/// Setup require for the service, before running the healthchecks and starting the service.
pub fn prepare_service(service_handler: &ServiceHandler) -> Result<(), std::io::Error> {
    if let Some(healthness) = &service_handler.service.healthness {
        if let Some(file_path) = &healthness.file_path {
            std::fs::remove_file(file_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::horust::formats::{Healthness, ServiceStatus};
    use crate::horust::{healthcheck, Service, ServiceHandler, Services};
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
    fn test_healthness_checks() {
        // _no_checks_needed
        let service = Service::get_sample_service().parse().unwrap();
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
        let mut service: Service = Service::get_sample_service().parse().unwrap();
        service.healthness = Some(healthcheck);
        let services = create_from_service(service);
        healthcheck::run_checks(&services);
        assert_status(&services, ServiceStatus::Starting);
        std::fs::write(filepath, "Hello world!").unwrap();
        healthcheck::run_checks(&services);
        assert_status(&services, ServiceStatus::Running);
    }
}
