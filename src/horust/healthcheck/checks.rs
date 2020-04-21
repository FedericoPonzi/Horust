use crate::horust::formats::Healthiness;
#[cfg(feature = "http-healthcheck")]
use reqwest::blocking::Client;

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
                        let client = Client::new();
                        let resp: reqwest::blocking::Response = client.head(endpoint).send().unwrap();
                        resp.status().is_success()
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
