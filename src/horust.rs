use crate::error::HorustError;
use crate::error::Result;
use crate::formats::{Service, ServiceName};
use libc::{_exit, STDOUT_FILENO};
use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, SIGCHLD};
use nix::sys::wait::waitpid;
use nix::unistd::{fork, getppid, ForkResult};
use nix::unistd::{getpid, Pid};
use std::collections::HashMap;
use std::ffi::{c_void, CStr, CString, OsStr};
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Horust {
    services: Vec<Vec<Service>>,
    running: HashMap<ServiceName, (Pid, Service)>,
}
impl Horust {
    pub fn new(services: Vec<Service>) -> super::error::Result<Self> {
        super::runtime::topological_sort(services).map(|exec_order| {
            println!("Exec order: {:?}", exec_order);
            Horust {
                services: exec_order,
                running: HashMap::new(),
            }
        })
    }
    pub fn run(&self) -> super::error::Result<()> {
        self.setup_signal_handling();

        self.services.iter().for_each(|services| {
            services.iter().for_each(|service| {
                self.spawn_process(service);
            });
            // \. Fork
            // 2. Save status, as in Started if needed,
            // 3. readiness check if needed
            // 4. continue looping.
        });
        Ok(())
    }

    pub fn from_services_dir<P>(path: &P) -> super::error::Result<Horust>
    where
        P: AsRef<Path> + ?Sized + AsRef<OsStr> + Debug,
    {
        Self::fetch_services(path)
            .map_err(Into::into)
            .and_then(|servs| {
                eprintln!("Servs found: {:?}", servs);
                Horust::new(servs)
            })
    }

    pub fn fetch_services<P>(path: &P) -> Result<Vec<Service>>
    where
        P: AsRef<Path> + ?Sized + AsRef<OsStr> + Debug,
    {
        eprintln!("Fetching services from : {:?}", path);
        fs::read_dir(path)
            .map_err(HorustError::from)
            .and_then(|dir| {
                dir.filter_map(std::result::Result::ok)
                    .map(|dir_entry| dir_entry.path())
                    .filter(|path: &PathBuf| {
                        /*eprintln!("Evaluating file: {:?}", path.display());
                        eprintln!(
                            "Result: {}, {}",
                            path.is_file(),
                            path.extension()
                                .unwrap_or("".as_ref())
                                .to_str()
                                .unwrap()
                                .ends_with("toml")
                        );*/
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

    pub fn run_service(&self, service: &Service) {
        eprintln!("Set cwd: {:?}", &service.working_directory);
        std::env::set_current_dir(&service.working_directory).unwrap();
        let mut chunks: Vec<&str> = service.command.split_whitespace().collect();
        let filename = CString::new(chunks.remove(0)).unwrap();

        let arg_cstrings = chunks
            .into_iter()
            .map(|arg| CString::new(arg).map_err(HorustError::from))
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let arg_cptr: Vec<&CStr> = arg_cstrings.iter().map(|c| c.as_c_str()).collect();
        eprintln!("Filepath: {:?}", filename);
        nix::unistd::execvp(filename.as_ref(), arg_cptr.as_ref()).expect("Execvp failed!!!");
    }

    pub fn spawn_process(&self, service: &Service) {
        match fork() {
            Ok(ForkResult::Child) => {
                println!("Child PID: {}, PPID: {}.", getpid(), getppid());
                self.run_service(service);
                //exit(123);
            }

            Ok(ForkResult::Parent { child, .. }) => {
                println!("Spawned child with PID {}.", child);
            }

            Err(err) => {
                panic!("fork() failed: {}", err);
            }
        };
    }

    fn setup_signal_handling(&self) {
        let sig_action = SigAction::new(
            SigHandler::Handler(Horust::handle_sigchld),
            SaFlags::empty(),
            SigSet::empty(),
        );

        if let Err(err) = unsafe { sigaction(SIGCHLD, &sig_action) } {
            panic!("[main] sigaction() failed: {}", err);
        };
    }
    extern "C" fn handle_sigchld(_: libc::c_int) {
        Horust::print_signal_safe("Received SIGCHILD.\n");
        match waitpid(Pid::from_raw(-1), None) {
            Ok(exit) => {
                // exit: WaitStatus has pid and exit code.
                Horust::print_signal_safe(format!("Child exited: {:?}.\n", exit).as_ref());
                Horust::exit_signal_safe(0);
            }
            Err(_) => {
                Horust::print_signal_safe("waitpid() failed.\n");
                Horust::exit_signal_safe(1);
            }
        }
    }
    fn print_signal_safe(s: &str) {
        unsafe {
            libc::write(STDOUT_FILENO, s.as_ptr() as *const c_void, s.len());
        }
    }

    fn exit_signal_safe(status: i32) {
        unsafe {
            _exit(status);
        }
    }
}

#[cfg(test)]
mod test {
    use crate::formats::Service;
    use std::io;
    use std::path::PathBuf;
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
