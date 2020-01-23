use crate::error::HorustError;
use crate::error::Result;
use crate::formats::ServiceStatus::Running;
use crate::formats::{RestartStrategy, Service, ServiceName, ServiceStatus};
use libc::{_exit, STDOUT_FILENO};
use libc::{prctl, PR_SET_CHILD_SUBREAPER};
use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, SIGCHLD};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{fork, getppid, ForkResult};
use nix::unistd::{getpid, Pid};
use std::collections::HashMap;
use std::ffi::{c_void, CStr, CString, OsStr};
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct ServiceHandler {
    service: Service,
    status: ServiceStatus,
    pid: Option<Pid>,
}
impl From<Service> for ServiceHandler {
    fn from(service: Service) -> Self {
        ServiceHandler {
            service,
            status: ServiceStatus::Stopped,
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

    fn supervisor_thread(supervised: Arc<Mutex<Vec<ServiceHandler>>>) {
        let mut reapable = HashMap::new();
        loop {
            match waitpid(Pid::from_raw(-1), None) {
                Ok(wait_status) => {
                    if let WaitStatus::Exited(pid, exit_code) = wait_status {
                        debug!("Pid has exited: {}", pid);
                        reapable.insert(pid, exit_code);
                        reapable = reapable
                            .into_iter()
                            .filter_map(|(pid, exit_code)| {
                                let mut locked = supervised.lock().unwrap();
                                debug!("{:?}", locked);
                                let service: Option<&mut ServiceHandler> = locked
                                    .iter_mut()
                                    .filter(|sh| sh.pid == Some(pid))
                                    .take(1)
                                    .last();
                                // It might happen that before supervised was updated, the process was already started, executed,
                                // and exited. Thus we're trying to reaping it, but there is still no map Pid -> Service.
                                if let Some(service) = service {
                                    match service.restart() {
                                        RestartStrategy::Never => {
                                            eprintln!("Pid successfully exited.");
                                            service.status = ServiceStatus::from_exit(exit_code);
                                            debug!("new locked: {:?}", locked);
                                        }
                                        RestartStrategy::OnFailure => {
                                            service.status = ServiceStatus::from_exit(exit_code);
                                            debug!("Going to rerun the process because it failed!");
                                        }
                                        RestartStrategy::Always => {
                                            service.status = ServiceStatus::Stopped;
                                        }
                                    }
                                    return None;
                                }
                                Some((pid, exit_code))
                            })
                            .collect();
                    }
                }
                Err(err) => {
                    if !err.to_string().contains("ECHILD") {
                        error!("Error waitpid(): {}", err);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(1))
        }
    }

    pub fn run(&mut self) -> super::error::Result<()> {
        unsafe {
            prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        }
        //self.setup_signal_handling();
        let supervised = Arc::clone(&self.supervised);
        std::thread::spawn(|| {
            Horust::supervisor_thread(supervised);
        });
        debug!("Going to start services!");
        loop {
            let mut sup = self.supervised.lock().unwrap();
            *sup = sup
                .iter()
                .cloned()
                .map(|mut service| {
                    let can_run = service
                        .start_after()
                        .iter()
                        .filter(|s| {
                            sup.iter()
                                .filter(|s| {
                                    s.status != ServiceStatus::Running
                                        && s.status != ServiceStatus::Stopped
                                })
                                .count()
                                != 0
                        })
                        .count()
                        == 0;
                    if can_run && service.status == ServiceStatus::Stopped {
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
                                        println!("Now it's running!");
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
            //let sup = self.supervised.lock().unwrap();
            let ret = sup
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

    pub fn from_services_dir<P>(path: &P) -> super::error::Result<Horust>
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
        fs::read_dir(path)
            .map_err(HorustError::from)
            .and_then(|dir| {
                dir.filter_map(std::result::Result::ok)
                    .map(|dir_entry| dir_entry.path())
                    .filter(|path: &PathBuf| {
                        path.is_file()
                            && path
                                .extension()
                                .unwrap_or_else(|| "".as_ref())
                                .to_str()
                                .unwrap()
                                .ends_with("toml")
                    })
                    .map(|path| {
                        fs::read_to_string(path)
                            .map_err(HorustError::from)
                            .and_then(|content| {
                                toml::from_str::<Service>(content.as_str())
                                    .map_err(HorustError::from)
                            })
                    })
                    .collect::<Result<Vec<Service>>>()
            })
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
            SigHandler::Handler(Horust::handle_sigchld),
            flags,
            SigSet::empty(),
        );

        if let Err(err) = unsafe { sigaction(SIGCHLD, &sig_action) } {
            panic!("sigaction() failed: {}", err);
        };
    }
    extern "C" fn handle_sigchld(_: libc::c_int) {
        SignalSafe::print("Received SIGCHILD.\n");
        match waitpid(Pid::from_raw(-1), None) {
            Ok(exit) => {
                // exit: WaitStatus has pid and exit code.

                SignalSafe::print(format!("Child exited: {:?}.\n", exit).as_ref());
                //Horust::exit_signal_safe(0);
            }
            Err(err) => {
                SignalSafe::print(format!("waitpid() failed {}.\n", err).as_ref());
                //Horust::exit_signal_safe(1);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::formats::Service;
    use std::io;
    use tempdir::TempDir;

    //TODO
    fn create_test_dir() -> io::Result<TempDir> {
        let ret = TempDir::new("horust").unwrap();
        let a = Service::from_name("a");
        let b = Service::start_after("b", vec!["a"]);

        let a_str = toml::to_string(&a).unwrap();
        let b_str = toml::to_string(&b).unwrap();
        std::fs::write(ret.path().join("my-first-service.yml"), a_str)?;
        std::fs::write(ret.path().join("my-second-service.yml"), b_str)?;

        Ok(ret)
    }
    fn test_fetch_directories() -> io::Result<()> {
        Ok(())
    }
}
