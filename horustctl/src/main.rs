use anyhow::{anyhow, bail, Result};
use clap::{Args, Parser, Subcommand};
use env_logger::Env;
use horust_commands_lib::{get_path, ClientHandler};
use log::debug;
use std::fs::read_dir;
use std::os::unix::fs::FileTypeExt;
use std::path::PathBuf;

/// Simple program to greet a person
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
    Status(StatusArgs),
}

#[derive(Args, Debug)]
struct StatusArgs {
    service_name: Option<String>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    let args = HorustctlArgs::parse();
    debug!("args: {args:?}");

    let uds_path = args.socket_path.unwrap_or_else(|| {
        get_uds_path(args.pid, args.uds_folder_path).expect("Failed to get uds_path.")
    });
    let mut uds_handler = ClientHandler::new_client(&uds_path)?;
    match &args.commands {
        Commands::Status(status_args) => {
            debug!("Status command received: {status_args:?}");
            debug!("uds path : {uds_path:?}");
            let (service_name, service_status) =
                uds_handler.send_status_request(status_args.service_name.clone().unwrap())?;
            println!(
                "Current status for '{service_name}' is: '{}'.",
                service_status.as_str_name()
            );
        }
    }
    Ok(())
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
                bail!("There is more than one socket in {sockets_folder_path:?}.Please use --pid to specify the pid of the horust process you want to talk to.");
            }
            sockets_folder_path.join(ret)
        }
        Some(pid) => get_path(&sockets_folder_path, pid),
    };
    debug!("Socket filename: {socket_path:?}");
    Ok(socket_path)
}
