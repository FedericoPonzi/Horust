use crate::horust::formats::ServiceStatus;
use crate::horust::{ServiceHandler, Services};
use std::path::PathBuf;

// TODO: this is not really healthness check, but rather readiness check. please change.
pub fn healthcheck_entrypoint(services: Services) {
    loop {
        // TODO
        let check_http_endpoint = |_endpoint: Option<&String>| true;
        let check_file = |path: Option<&PathBuf>| path.is_some() && path.unwrap().exists();

        // if no healthness needs to be performed, set as running.
        services
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|sh| sh.service.healthness.is_none())
            .for_each(|sh| {
                if sh.is_to_be_run() {
                    sh.set_status(ServiceStatus::Running)
                }
            });
        services
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|sh| {
                let healthness = sh.service.healthness.as_ref();
                let ret = sh.is_starting() && healthness.is_some();
                if ret {
                    let healthness = healthness.unwrap();
                    healthness.file_path.is_some() || healthness.http_endpoint.is_some()
                } else {
                    ret
                }
            })
            .filter(|sh| {
                let checks = sh.service.healthness.as_ref().unwrap();
                vec![
                    check_file(checks.file_path.as_ref()),
                    check_http_endpoint(checks.http_endpoint.as_ref()),
                ]
                .into_iter()
                .any(|val| val)
            })
            .for_each(|sh| {
                // Maybe the service has already exited or failed.
                if sh.is_starting() {
                    debug!("since it's not to be run, let's set it to running! :D");
                    sh.status = ServiceStatus::Running
                }
            });
    }
}

pub fn prepare_service(service_handler: &ServiceHandler) -> Result<(), std::io::Error> {
    if let Some(healthness) = &service_handler.service.healthness {
        if let Some(file_path) = &healthness.file_path {
            std::fs::remove_file(file_path)?;
        }
    }
    Ok(())
}
