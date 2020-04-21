use crate::horust::formats::Healthiness;
#[cfg(feature = "http-healthcheck")]
use reqwest::blocking::Client;
use std::time::Duration;

pub(crate) trait Check {
    fn run(&self, healthiness: &Healthiness) -> bool;
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
}
