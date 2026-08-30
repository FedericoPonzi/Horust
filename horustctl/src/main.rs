use anyhow::{Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use env_logger::Env;
use horust_commands_lib::{ClientHandler, HorustMsgServiceStatus, get_path};
use log::debug;
use std::fs::read_dir;
use std::os::unix::fs::FileTypeExt;
use std::path::PathBuf;

/// CLI tool for managing horust services
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct HorustctlArgs {
    /// The pid of the horust process you want to query. Optional if only one horust is running in the system.
    #[arg(short, long)]
    pid: Option<i32>,

    #[arg(short, long, default_value = "/var/run/horust/")]
    uds_folder_path: PathBuf,

    // Specify the full path of the socket. It takes precedence other over arguments.
    #[arg(long)]
    socket_path: Option<PathBuf>,

    #[command(subcommand)]
    commands: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show the status of one or all services
    Status(StatusArgs),
    /// Start a stopped service
    Start(ServiceNameArg),
    /// Stop a running service
    Stop(ServiceNameArg),
    /// Restart a service (stop then start)
    Restart(ServiceNameArg),
}

#[derive(Args, Debug)]
struct StatusArgs {
    /// Service name. If omitted, shows all services.
    service_name: Option<String>,
}

#[derive(Args, Debug)]
struct ServiceNameArg {
    /// The name of the service
    service_name: String,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let args = HorustctlArgs::parse();
    debug!("args: {args:?}");

    let uds_path = args.socket_path.unwrap_or_else(|| {
        get_uds_path(args.pid, args.uds_folder_path).expect("Failed to get uds_path.")
    });
    let mut uds_handler = ClientHandler::new_client(&uds_path)?;
    match &args.commands {
        Commands::Status(status_args) => {
            debug!("Status command received: {status_args:?}");
            match &status_args.service_name {
                Some(name) => {
                    let (service_name, service_status) =
                        uds_handler.send_status_request(name.clone())?;
                    println!("{:<30} {}", service_name, format_status(&service_status));
                }
                None => {
                    let statuses = uds_handler.send_all_status_request()?;
                    if statuses.is_empty() {
                        println!("No services found.");
                    } else {
                        println!("{:<30} STATUS", "SERVICE");
                        for (name, status) in &statuses {
                            println!("{:<30} {}", name, format_status(status));
                        }
                    }
                }
            }
        }
        Commands::Start(arg) => {
            let (name, accepted) = uds_handler
                .send_change_request(arg.service_name.clone(), HorustMsgServiceStatus::Initial)?;
            if accepted {
                println!("Start command accepted for '{name}'.");
            } else {
                println!("Start command rejected for '{name}'.");
            }
        }
        Commands::Stop(arg) => {
            let (name, accepted) = uds_handler
                .send_change_request(arg.service_name.clone(), HorustMsgServiceStatus::Inkilling)?;
            if accepted {
                println!("Stop command accepted for '{name}'.");
            } else {
                println!("Stop command rejected for '{name}'.");
            }
        }
        Commands::Restart(arg) => {
            let (name, accepted) = uds_handler.send_restart_request(arg.service_name.clone())?;
            if accepted {
                println!("Restart command accepted for '{name}'.");
            } else {
                println!("Restart command rejected for '{name}'.");
            }
        }
    }
    Ok(())
}

fn format_status(status: &HorustMsgServiceStatus) -> &'static str {
    match status {
        HorustMsgServiceStatus::Starting => "STARTING",
        HorustMsgServiceStatus::Started => "STARTED",
        HorustMsgServiceStatus::Running => "RUNNING",
        HorustMsgServiceStatus::Inkilling => "STOPPING",
        HorustMsgServiceStatus::Success => "SUCCESS",
        HorustMsgServiceStatus::Finished => "FINISHED",
        HorustMsgServiceStatus::Finishedfailed => "FAILED (finished)",
        HorustMsgServiceStatus::Failed => "FAILED",
        HorustMsgServiceStatus::Initial => "INITIAL",
    }
}

fn get_uds_path(pid: Option<i32>, sockets_folder_path: PathBuf) -> Result<PathBuf> {
    if !sockets_folder_path.exists() {
        bail!("the specified sockets folder path '{sockets_folder_path:?}' does not exists.");
    }
    if !sockets_folder_path.is_dir() {
        bail!("the specified sockets folder path '{sockets_folder_path:?}' is not a directory.");
    }

    let socket_path = match pid {
        None => {
            let mut readdir_iter = read_dir(&sockets_folder_path)?
                .filter_map(|d| d.ok()) // unwrap results
                .filter_map(|d| -> Option<String> {
                    let is_socket = d.file_type().ok()?.is_socket();
                    let name = d.file_name();
                    let name = name.to_string_lossy();
                    if is_socket && name.starts_with("horust-") && name.ends_with(".sock") {
                        Some(name.to_string())
                    } else {
                        None
                    }
                });

            let ret = readdir_iter
                .next()
                .ok_or_else(|| anyhow!("No socket found in {sockets_folder_path:?}"))?;
            if readdir_iter.count() > 0 {
                bail!(
                    "There is more than one socket in {sockets_folder_path:?}.Please use --pid to specify the pid of the horust process you want to talk to."
                );
            }
            sockets_folder_path.join(ret)
        }
        Some(pid) => get_path(&sockets_folder_path, pid),
    };
    debug!("Socket filename: {socket_path:?}");
    Ok(socket_path)
}
