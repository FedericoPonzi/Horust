use crate::horust::formats::ServiceStatus;
use crate::horust::{ServiceHandler, Services};

// TODO: this is not really healthness check, but rather readiness check. please change.
pub fn healthcheck_entrypoint(services: Services) {
    loop {
        run_checks(&services)
    }
}
fn check_http_endpoint(_endpoint: &str) -> bool {
    false
}

fn run_checks(services: &Services) {
    // TODO
    services
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|sh| sh.is_starting())
        .filter(|sh| match sh.service.healthness.as_ref() {
            Some(healthness) => {
                let mut ret = false;
                if let Some(file_path) = healthness.file_path.as_ref() {
                    ret = file_path.exists();
                }
                if let Some(endpoint) = healthness.http_endpoint.as_ref() {
                    ret = check_http_endpoint(endpoint);
                }
                /*
                    Edge case: [healthcheck] header section is defined, but then it's empty. This should pass.
                */
                let empty_section =
                    healthness.file_path.is_some() || healthness.http_endpoint.is_some();

                ret || !empty_section
            }
            None => true,
        })
        .for_each(|sh| sh.status = ServiceStatus::Running);
}

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
