use crate::horust::formats::Healthiness;
#[cfg(feature = "http-healthcheck")]
use reqwest::blocking::Client;
use std::time::Duration;

static FILE_CHECK: FilePathCheck = FilePathCheck {};
static HTTP_CHECK: HttpCheck = HttpCheck {};

pub(crate) fn get_checks() -> Vec<&'static dyn Check> {
    let checks: Vec<&dyn Check> = vec![&FILE_CHECK, &HTTP_CHECK];
    checks
}

pub(crate) trait Check {
    fn run(&self, healthiness: &Healthiness) -> bool;
    fn prepare(&self, _healtiness: &Healthiness) -> Result<(), std::io::Error> {
        Ok(())
    }
}

pub(crate) struct HttpCheck;

impl Check for HttpCheck {
    fn run(&self, healthiness: &Healthiness) -> bool {
        healthiness
            .http_endpoint.as_ref()
            .map(|endpoint| {
                if cfg!(not(feature = "http-healthcheck")){
                    error!("There is an http based healthcheck, but horust was built without the http-healthcheck feature (thus it will never pass these checks).");
                    return false;
                }
                #[cfg(feature = "http-healthcheck")]
                    {
                        let client = Client::builder()
                            .timeout(Duration::from_secs(1))
                            .build().expect("Http client");
                        let resp: Result<reqwest::blocking::Response, reqwest::Error> = client.head(endpoint).send();
                        resp.map(|resp| resp.status().is_success()).unwrap_or(false)
                    }
            })
            .unwrap_or(true)
    }
}

pub(crate) struct FilePathCheck;

impl Check for FilePathCheck {
    fn run(&self, healthiness: &Healthiness) -> bool {
        healthiness
            .file_path
            .as_ref()
            .map(|file_path| file_path.exists())
            .unwrap_or(true)
    }
    fn prepare(&self, healthiness: &Healthiness) -> Result<(), std::io::Error> {
        //TODO: check if user has permissions to remove this file.
        if let Some(file_path) = healthiness.file_path.as_ref() {
            std::fs::remove_file(file_path)
        } else {
            Ok(())
        }
    }
}
