mod bus;
mod error;
mod formats;
mod healthcheck;
mod signal_safe;
mod supervisor;

pub use self::error::HorustError;
pub use self::formats::{get_sample_service, ExitStatus, HorustConfig};
use crate::horust::bus::Bus;
use crate::horust::error::Result;
use crate::horust::formats::{validate, Service};
pub use formats::Event;
use libc::{prctl, PR_SET_CHILD_SUBREAPER};
use std::ffi::OsStr;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Horust {
    services: Vec<Service>,
    services_dir: Option<PathBuf>,
}

impl Horust {
    fn new(services: Vec<Service>, services_dir: Option<PathBuf>) -> Self {
        Horust {
            services,
            services_dir,
        }
    }

    pub fn get_services(&self) -> &Vec<Service> {
        &self.services
    }
    /// Creates a new Horust instance from a command.
    /// The command will be wrapped in a service and run with sane defaults
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

    /// Blocking call, will setup the event loop and the threads and run all the available services.
    pub fn run(&mut self) -> ExitStatus {
        // sets Horust as a subreaper. From prctl(2):
        // A subreaper fulfills the role of init(1) for its descendant processes. Upon termination
        // of a process that is orphaned (i.e., its immediate parent has already terminated) and
        // marked as having a subreaper, the nearest still living ancestor subreaper will receive a
        // SIGCHLD signal and be able to wait(2) on the process to discover its termination status.
        unsafe {
            prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        }
        supervisor::init();

        let mut dispatcher = Bus::new(true);
        debug!("Services: {:?}", self.services);
        // Spawn helper threads:
        healthcheck::spawn(dispatcher.join_bus(), self.services.clone());
        let handle = supervisor::spawn(dispatcher.join_bus(), self.services.clone());
        dispatcher.run();
        handle.join().unwrap()
    }
}

/// Search for *.toml files in path, and deserialize them into Service.
fn fetch_services<P>(path: &P) -> Result<Vec<Service>>
where
    P: AsRef<Path> + ?Sized + AsRef<OsStr> + Debug,
{
    debug!("Fetching services from : {:?}", path);
    let has_toml_extension = |path: &PathBuf| {
        path.extension()
            .unwrap_or_else(|| "".as_ref())
            .to_str()
            .unwrap_or("")
            .ends_with("toml")
    };
    let is_toml_file = |path: &PathBuf| path.is_file() && has_toml_extension(path);
    let dir = fs::read_dir(path)?;

    //TODO: option to decide to not start if the deserialization of any service failed.
    let services = dir
        .filter_map(std::result::Result::ok)
        .map(|dir_entry| dir_entry.path())
        .filter(is_toml_file)
        .map(Service::from_file)
        .filter_map(|res| {
            if let Err(error) = &res {
                error!("Error loading toml file: {}", error);
            }
            res.ok()
        })
        .collect::<Vec<Service>>();
    if services.is_empty() {
        error!("Horust: No valid services found in: {:?}", path);
    }
    Ok(services)
}

#[cfg(test)]
mod test {
    use crate::horust::fetch_services;
    use crate::horust::formats::Service;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use tempdir::TempDir;

    /// List files in path, filtering out directories
    fn list_files<P: AsRef<Path>>(path: P) -> std::io::Result<Vec<PathBuf>> {
        fs::read_dir(path)?
            .filter_map(|entry| entry.ok())
            .try_fold(vec![], |mut ret, entry| {
                entry.file_type().map(|ftype| {
                    if ftype.is_file() {
                        ret.push(entry.path());
                    }
                    ret
                })
            })
    }

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

    #[test]
    fn test_list_files() -> io::Result<()> {
        let tempdir = TempDir::new("horust").unwrap();
        let files = vec!["a", "b", "c"];
        let files: Vec<PathBuf> = files
            .into_iter()
            .map(|f| tempdir.path().join(f).to_path_buf())
            .collect();

        for f in &files {
            std::fs::write(f, "Hello world")?;
        }
        let dirs = vec!["1", "2", "3"];
        for d in dirs {
            std::fs::create_dir(tempdir.path().join(d))?;
        }
        let mut res = list_files(tempdir.path())?;
        res.sort();
        assert_eq!(res, files);

        Ok(())
    }
}
