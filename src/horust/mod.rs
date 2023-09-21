use std::ffi::OsStr;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
#[cfg(target_os = "linux")]
use libc::{prctl, PR_SET_CHILD_SUBREAPER};

pub use formats::Event;

use crate::horust::bus::Bus;
use crate::horust::formats::{validate, Service};

pub use self::formats::{get_sample_service, ExitStatus, HorustConfig};

mod bus;
mod error;
mod formats;
mod healthcheck;
mod signal_safe;
mod supervisor;

#[derive(Debug)]
pub struct Horust {
    services: Vec<Service>,
}

impl Horust {
    fn new(services: Vec<Service>) -> Self {
        Horust { services }
    }

    pub fn get_services(&self) -> &[Service] {
        &self.services
    }
    /// Creates a new Horust instance from a command.
    /// The command will be wrapped in a service and run with sane defaults
    pub fn from_command(command: String) -> Self {
        Self::new(vec![Service::from_command(command)])
    }

    /// Create a new horust instance from multiple paths of services.
    pub fn from_services_dirs(paths: &[PathBuf]) -> Result<Self> {
        let services = paths
            .iter()
            .map(|path| fetch_services(path))
            .flat_map(|result| match result {
                Ok(vec) => vec.into_iter().map(Ok).collect(),
                Err(err) => vec![Err(err)],
            })
            .collect::<Result<Vec<_>>>()?;

        let services = validate(services)?;
        Ok(Horust::new(services))
    }

    /// Blocking call, will setup the event loop and the threads and run all the available services.
    pub fn run(&mut self) -> ExitStatus {
        #[cfg(target_os = "linux")]
        unsafe {
            // A subreaper fulfills the role of init(1) for its
            // descendant processes.  When a process becomes orphaned
            // (i.e., its immediate parent terminates), then that process
            // will be reparented to the nearest still living ancestor
            // subreaper.  Subsequently, calls to getppid(2) in the
            // orphaned process will now return the PID of the subreaper
            // process, and when the orphan terminates, it is the
            // subreaper process that will receive a SIGCHLD signal and
            // will be able to wait(2) on the process to discover its
            // termination status.
            // https://man7.org/linux/man-pages/man2/prctl.2.html
            prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        }
        supervisor::init();

        let dispatcher = Bus::new();
        debug!("Services: {:?}", self.services);
        // Spawn helper threads:
        healthcheck::spawn(dispatcher.join_bus(), self.services.clone());
        let handle = supervisor::spawn(dispatcher.join_bus(), self.services.clone());
        dispatcher.run();
        handle.join().unwrap()
    }
}

fn load_service<P>(path: P) -> Result<Service>
where
    P: AsRef<Path> + Sized + AsRef<OsStr> + Debug,
    PathBuf: From<P>,
{
    let res = Service::from_file(&path);
    let path = PathBuf::from(path);
    res.map(|mut service| {
        if service.name.is_empty() {
            let filename = path.file_name().unwrap().to_str().unwrap().to_owned();
            service.name = filename;
        }
        service
    })
    .map_err(|error| {
        let error = error.context(format!("Failed loading toml file: {:?}", path.display()));
        error!("{:?}", error);
        error
    })
}

fn is_toml_file(path: &Path) -> bool {
    let has_toml_extension = |path: &Path| {
        path.extension()
            .unwrap_or_else(|| "".as_ref())
            .to_str()
            .unwrap_or("")
            .ends_with("toml")
    };
    path.is_file() && has_toml_extension(path)
}

// TODO: option to decide to not start if the deserialization of any service failed.
/// Search for *.toml files in path, and deserialize them into Service.
fn fetch_services(path: &Path) -> Result<Vec<Service>> {
    debug!("Fetching services from: {}", path.display());

    let paths = if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        fs::read_dir(path)?
            .inspect(|p| {
                if let Err(err) = p {
                    error!("Error loading entry: {}", err);
                }
            })
            .filter_map(Result::ok)
            .map(|direntry| direntry.path())
            .collect()
    };
    let services = paths
        .into_iter()
        .filter(|p| is_toml_file(p))
        .map(load_service)
        .filter_map(Result::ok)
        .collect::<Vec<Service>>();
    if services.is_empty() {
        error!("Horust: No (valid) services found in: {}", path.display());
    }
    Ok(services)
}

#[cfg(test)]
mod test {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    use tempdir::TempDir;

    use crate::horust::fetch_services;
    use crate::horust::formats::Service;

    const FIRST_SERVICE_FILENAME: &str = "my-first-service.toml";
    const SECOND_SERVICE_FILENAME: &str = "my-second-service.toml";

    /// List files in path, filtering out directories
    fn list_files<P: AsRef<Path>>(path: P) -> io::Result<Vec<PathBuf>> {
        fs::read_dir(path)?
            .filter_map(Result::ok)
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
        fs::write(ret.path().join(FIRST_SERVICE_FILENAME), a_str)?;
        fs::write(ret.path().join(SECOND_SERVICE_FILENAME), b_str)?;
        Ok(ret)
    }

    #[test]
    fn test_fetch_services() -> io::Result<()> {
        let tempdir = TempDir::new("horust").unwrap();
        // Empty service directory will print a log but it's not an error.
        assert_eq!(fetch_services(tempdir.path()).unwrap().len(), 0);
        let not_toml_file = tempdir.path().join("not_a_toml.toml");
        fs::write(not_toml_file.clone(), "not really a toml.")?;

        // Today Horust filters out invalid toml files.
        assert_eq!(fetch_services(tempdir.path()).unwrap().len(), 0);

        // This is not a directory
        assert_eq!(fetch_services(&not_toml_file).unwrap().len(), 0);

        let tempdir = create_test_dir()?;
        fs::write(tempdir.path().join("not-a-service"), "Hello world")?;
        let res = fetch_services(tempdir.path()).unwrap();
        assert_eq!(res.len(), 2,);
        let mut names: Vec<String> = res.into_iter().map(|serv| serv.name).collect();
        names.sort();
        assert_eq!(vec!["a", "b"], names);

        // Load a service from a single file instead of a directory
        let res = fetch_services(&tempdir.path().join(FIRST_SERVICE_FILENAME)).unwrap();
        assert_eq!(res.len(), 1,);

        Ok(())
    }

    #[test]
    fn test_list_files() -> io::Result<()> {
        let tempdir = TempDir::new("horust").unwrap();
        let files = vec!["a", "b", "c"];
        let files: Vec<PathBuf> = files.into_iter().map(|f| tempdir.path().join(f)).collect();

        for f in &files {
            fs::write(f, "Hello world")?;
        }
        let dirs = vec!["1", "2", "3"];
        for d in dirs {
            fs::create_dir(tempdir.path().join(d))?;
        }
        let mut res = list_files(tempdir.path())?;
        res.sort();
        assert_eq!(res, files);

        Ok(())
    }
}
