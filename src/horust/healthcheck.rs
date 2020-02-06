use crate::horust::error::ErrorKind::SerDe;
use crate::horust::formats::ServiceStatus;
use crate::horust::{Service, ServiceHandler, Services};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub fn health_check(services: Services) {
    // TODO
    let check_http_endpoint = |endpoint: Option<&String>| ServiceStatus::Starting;
    let check_file = |path: Option<&PathBuf>| {
        if path.is_some() && path.unwrap().exists() {
            ServiceStatus::Running
        } else {
            ServiceStatus::Starting
        }
    };
    services
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|sh| {
            let healthness = sh.service.healthness.as_ref();
            sh.is_starting() && healthness.is_some() && healthness.unwrap().file_path.is_some()
                || healthness.unwrap().http_endpoint.is_some()
        })
        .filter(|sh| {
            let checks = sh.service.healthness.as_ref().unwrap();
            vec![
                check_file(checks.file_path.as_ref()),
                check_http_endpoint(checks.http_endpoint.as_ref()),
            ]
            .into_iter()
            .any(|val| val == ServiceStatus::Running)
        })
        .for_each(|sh| sh.status = ServiceStatus::Running);
}
