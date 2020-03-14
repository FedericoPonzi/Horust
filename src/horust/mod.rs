mod bus;
mod error;
mod formats;
mod healthcheck;
mod reaper;
mod repository;
mod runtime;
mod signal_handling;

pub use self::error::HorustError;
pub use self::formats::get_sample_service;
use crate::horust::bus::Bus;
use crate::horust::error::Result;
use crate::horust::formats::{validate, Service};
use crate::horust::repository::ServiceRepository;
use libc::{prctl, PR_SET_CHILD_SUBREAPER};
use std::ffi::OsStr;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Horust {
    pub services: Vec<Service>,
    services_dir: Option<PathBuf>,
}

impl Horust {
    fn new(services: Vec<Service>, services_dir: Option<PathBuf>) -> Self {
        Horust {
            services,
            services_dir,
        }
    }

    pub fn from_command(command: String) -> Self {
        Self::new(vec![Service::from_command(command)], None)
    }

    /// Create a new horust instance from a path of services.
    pub fn from_services_dir<P>(path: &P) -> Result<Self>
    where
        P: AsRef<Path> + ?Sized + AsRef<OsStr> + Debug,
    {
        let services = fetch_services(&path)?;
        validate(services)
            .map_err(Into::into)
            .map(|services| Horust::new(services, Some(PathBuf::from(path))))
    }

    pub fn run(&mut self) {
        unsafe {
            prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        }
        signal_handling::init();

        let mut dispatcher = Bus::new();
        let mut new_service_repo =
            || ServiceRepository::new(self.services.clone(), dispatcher.join_bus());
        debug!("Services: {:?}", self.services);
        // Spawn helper threads:
        debug!("Going to spawn threads:, going to start running services now!");
        runtime::spawn(new_service_repo());
        reaper::spawn(new_service_repo());
        healthcheck::spawn(new_service_repo());
        dispatcher.run();
    }
}

pub fn list_files<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut paths = std::fs::read_dir(path)?;
    paths.try_fold(vec![], |mut ret, p| match p {
        Ok(entry) => {
            ret.push(entry.path());
            Ok(ret)
        }
        Err(err) => Err(err),
    })
}

/// Search for *.toml files in path, and deserialize them into Service.
fn fetch_services<P>(path: &P) -> Result<Vec<Service>>
where
    P: AsRef<Path> + ?Sized + AsRef<OsStr> + Debug,
{
    debug!("Fetching services from : {:?}", path);
    debug!("Files: {:?}", list_files(path));
    let is_toml_file = |path: &PathBuf| {
        let has_toml_extension = |path: &PathBuf| {
            path.extension()
                .unwrap_or_else(|| "".as_ref())
                .to_str()
                .unwrap()
                .ends_with("toml")
        };
        path.is_file() && has_toml_extension(path)
    };
    let dir = fs::read_dir(path)?;

    //TODO: option to decide to not start if the deserialization of any service failed.
    let services = dir
        .filter_map(std::result::Result::ok)
        .map(|dir_entry| dir_entry.path())
        .filter(is_toml_file)
        .map(|file| {
            let res = Service::from_file(&file);
            res.map(|mut service| {
                if service.name == "" {
                    let filename = file.file_name().unwrap().to_str().unwrap().to_owned();
                    service.name = filename;
                }
                service
            })
            .map_err(|error| {
                error!("Error loading toml file: {}", error);
                error
            })
        })
        .filter(Result::is_ok)
        .map(Result::unwrap)
        .collect::<Vec<Service>>();
    if services.is_empty() {
        println!("Horust: No services found in: {:?}.", path);
    }
    Ok(services)
}

#[cfg(test)]
mod test {
    use crate::horust::fetch_services;
    use crate::horust::formats::Service;
    use std::io;
    use tempdir::TempDir;

    fn create_test_dir() -> io::Result<TempDir> {
        let ret = TempDir::new("horust").unwrap();
        let a = Service::from_name("a");
        let b = Service::start_after("b", vec!["a"]);
        let a_str = toml::to_string(&a).unwrap();
        let b_str = toml::to_string(&b).unwrap();
        std::fs::write(ret.path().join("my-first-service.toml"), a_str)?;
        std::fs::write(ret.path().join("my-second-service.toml"), b_str)?;
        Ok(ret)
    }

    #[test]
    fn test_fetch_services() -> io::Result<()> {
        let tempdir = create_test_dir()?;
        std::fs::write(tempdir.path().join("not-a-service"), "Hello world")?;
        let res = fetch_services(tempdir.path()).unwrap();
        assert_eq!(res.len(), 2);
        let mut names: Vec<String> = res.into_iter().map(|serv| serv.name).collect();
        names.sort();
        assert_eq!(vec!["a", "b"], names);

        Ok(())
    }
}
