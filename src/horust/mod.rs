mod error;
mod formats;
mod reaper;

pub use self::error::HorustError;
use self::error::Result;
use self::formats::{RestartStrategy, Service, ServiceName, ServiceStatus};
use libc::{_exit, STDOUT_FILENO};
use libc::{prctl, PR_SET_CHILD_SUBREAPER};
use nix::sys::signal::{sigaction, signal, SaFlags, SigAction, SigHandler, SigSet, SIGTERM};
use nix::sys::wait::WaitStatus;
use nix::unistd::{fork, getppid, ForkResult};
use nix::unistd::{getpid, Pid};
use std::ffi::{c_void, CStr, CString, OsStr};
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

struct SignalSafe;
impl SignalSafe {
    fn print(s: &str) {
        unsafe {
            libc::write(STDOUT_FILENO, s.as_ptr() as *const c_void, s.len());
        }
    }

    fn exit(status: i32) {
        unsafe {
            _exit(status);
        }
    }
}

static mut SIGTERM_RECEIVED: bool = false;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceHandler {
    service: Service,
    status: ServiceStatus,
    pid: Option<Pid>,
}
impl From<Service> for ServiceHandler {
    fn from(service: Service) -> Self {
        ServiceHandler {
            service,
            status: ServiceStatus::Initial,
            pid: None,
        }
    }
}
impl From<ServiceHandler> for Service {
    fn from(sh: ServiceHandler) -> Self {
        sh.service
    }
}
impl ServiceHandler {
    fn start_after(&self) -> &Vec<String> {
        self.service.start_after.as_ref()
    }
    fn name(&self) -> &str {
        self.service.name.as_str()
    }
    fn restart(&self) -> &RestartStrategy {
        &self.service.restart
    }
}

#[derive(Debug)]
pub struct Horust {
    supervised: Arc<Mutex<Vec<ServiceHandler>>>,
}

impl Horust {
    pub fn new(services: Vec<Service>) -> Self {
        Horust {
            supervised: Arc::new(Mutex::new(
                services
                    .clone()
                    .into_iter()
                    .map(ServiceHandler::from)
                    .collect(),
            )),
        }
    }

    pub fn run(&mut self) -> Result<()> {
        unsafe {
            prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        }
        //self.setup_signal_handling();
        let supervised = Arc::clone(&self.supervised);
        std::thread::spawn(|| {
            reaper::supervisor_thread(supervised);
        });
        debug!("Going to start services!");
        loop {
            let mut superv_services = self.supervised.lock().unwrap();
            *superv_services = superv_services
                .iter()
                .cloned()
                .map(|mut service| {
                    /// Check if all dependant services are either running or finished:
                    let can_run = service
                        .start_after()
                        .iter()
                        .filter(|service_name| {
                            // Looking for the supervised services:
                            superv_services
                                .iter()
                                .filter(|s| {
                                    &s.service.name == *service_name
                                        && s.status != ServiceStatus::Running
                                        && s.status != ServiceStatus::Finished
                                })
                                .count()
                                != 0
                        })
                        .count()
                        == 0;
                    if can_run && service.status == ServiceStatus::Initial {
                        service.status = ServiceStatus::ToBeRun;
                        let supervised_ref = Arc::clone(&self.supervised);
                        let service = service.service.clone();
                        std::thread::spawn(move || {
                            let pid =
                                Horust::run_service(&service).expect("Failed spawning service!");
                            let supervised_ref = Arc::clone(&supervised_ref);
                            let mut sup = supervised_ref.lock().unwrap();
                            *sup = sup
                                .iter()
                                .cloned()
                                .map(|mut sh| {
                                    if sh.name() == service.name {
                                        debug!("Now it's running!");
                                        sh.status = ServiceStatus::Running;
                                        sh.pid = Some(pid);
                                    }
                                    sh
                                })
                                .collect();
                        });
                    }
                    service
                })
                .collect();
            let ret = superv_services
                .iter()
                .filter(|sh| sh.status != ServiceStatus::Finished)
                .count();

            // Every process has finished:
            if ret == 0 {
                break;
            }
        }
        Ok(())
    }

    pub fn run_service(service: &Service) -> Result<Pid> {
        std::thread::sleep(service.start_delay);
        Horust::spawn_process(service)
    }

    pub fn from_services_dir<P>(path: &P) -> Result<Horust>
    where
        P: AsRef<Path> + ?Sized + AsRef<OsStr> + Debug,
    {
        Self::fetch_services(path).map_err(Into::into).map(|servs| {
            debug!("Services found: {:?}", servs);
            Horust::new(servs)
        })
    }

    /// Search for *.toml files in path, and deserialize them into Service.
    pub fn fetch_services<P>(path: &P) -> Result<Vec<Service>>
    where
        P: AsRef<Path> + ?Sized + AsRef<OsStr> + Debug,
    {
        debug!("Fetching services from : {:?}", path);
        let dir = fs::read_dir(path)?;
        dir.filter_map(std::result::Result::ok)
            .map(|dir_entry| dir_entry.path())
            .filter(|path: &PathBuf| {
                let has_toml_extension = |path: &PathBuf| {
                    path.extension()
                        .unwrap_or_else(|| "".as_ref())
                        .to_str()
                        .unwrap()
                        .ends_with("toml")
                };
                path.is_file() && has_toml_extension(path)
            })
            .map(|path| {
                fs::read_to_string(path)
                    .map_err(HorustError::from)
                    .and_then(|content| {
                        toml::from_str::<Service>(content.as_str()).map_err(HorustError::from)
                    })
            })
            .collect::<Result<Vec<Service>>>()
    }

    pub fn spawn_process(service: &Service) -> Result<Pid> {
        match fork() {
            Ok(ForkResult::Child) => {
                debug!("Child PID: {}, PPID: {}.", getpid(), getppid());
                Horust::exec_service(service);
                unreachable!()
            }

            Ok(ForkResult::Parent { child, .. }) => {
                debug!("Spawned child with PID {}.", child);
                return Ok(child);
            }

            Err(err) => {
                return Err(HorustError::from(err));
            }
        }
    }
    pub fn exec_service(service: &Service) {
        debug!("Set cwd: {:?}", &service.working_directory);
        std::env::set_current_dir(&service.working_directory).unwrap();
        let mut chunks: Vec<&str> = service.command.split_whitespace().collect();
        let filename = CString::new(chunks.remove(0)).unwrap();

        let mut arg_cstrings = chunks
            .into_iter()
            .map(|arg| CString::new(arg).map_err(HorustError::from))
            .collect::<Result<Vec<_>>>()
            .unwrap();
        arg_cstrings.insert(0, filename.clone());
        debug!("args: {:?}", arg_cstrings);
        let arg_cptr: Vec<&CStr> = arg_cstrings.iter().map(|c| c.as_c_str()).collect();
        // TODO: clear signal mask if needed.
        nix::unistd::execvp(filename.as_ref(), arg_cptr.as_ref()).expect("Execvp() failed: ");
    }

    fn disable_signal_handling(&self) {
        nix::sys::signal::sigprocmask(
            nix::sys::signal::SigmaskHow::SIG_BLOCK,
            Some(&SigSet::all()),
            None,
        )
        .expect("Failed to set sigprocmask.");
    }
    fn setup_signal_handling(&self) {
        self.disable_signal_handling();

        // To allow auto restart on some syscalls,
        // for example: `waitpid`.
        let flags = SaFlags::SA_RESTART;
        let sig_action = SigAction::new(
            SigHandler::Handler(Horust::handle_sigterm),
            flags,
            SigSet::empty(),
        );

        if let Err(err) = unsafe { sigaction(SIGTERM, &sig_action) } {
            panic!("sigaction() failed: {}", err);
        };
    }
    //TODO: kill -9 -1
    extern "C" fn handle_sigterm(_: libc::c_int) {
        SignalSafe::print("Received SIGTERM.\n");
        unsafe {
            SIGTERM_RECEIVED = true;
        }
        SignalSafe::exit(1);
    }
}

#[cfg(test)]
mod test {
    use crate::horust::formats::Service;
    use crate::horust::Horust;
    use std::io;
    use tempdir::TempDir;

    //TODO
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
        let res = Horust::fetch_services(tempdir.path()).unwrap();
        assert_eq!(res.len(), 2);
        let names: Vec<String> = res.into_iter().map(|serv| serv.name).collect();
        assert_eq!(vec!["a", "b"], names);

        Ok(())
    }
}
