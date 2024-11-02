use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use env_logger::Env;
use log::debug;
use std::env;
use std::fs::read_dir;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct HourstctlArgs {
    /// Optional if only one horust is running in the system.
    #[arg(short, long)]
    pid: Option<u32>,

    #[arg(short, long, default_value = "/var/run/horust/")]
    sockets_folder_path: PathBuf,

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
    let args = HourstctlArgs::parse();
    debug!("args: {args:?}");

    let uds_path = get_uds_path(args.pid, args.sockets_folder_path)?;
    match &args.commands {
        Commands::Status(status_args) => {
            debug!("Status command received: {status_args:?}");
            debug!("uds path : {uds_path:?}")
        }
    }
    Ok(())
}

fn get_uds_path(pid: Option<u32>, sockets_folder_path: PathBuf) -> Result<PathBuf> {
    if !sockets_folder_path.exists() {
        bail!("the specified sockets folder path '{sockets_folder_path:?}' does not exists.");
    }
    if !sockets_folder_path.is_dir() {
        bail!("the specified sockets folder path '{sockets_folder_path:?}' is not a directory.");
    }

    let socket_file_name = if pid.is_none() {
        let mut readdir_iter = read_dir(&sockets_folder_path)?;
        let ret = readdir_iter
            .next()
            .unwrap()? // check if it's there.
            .file_name()
            .to_string_lossy()
            .to_string();
        if readdir_iter.count() > 0 {
            bail!("There is more than one socket in {sockets_folder_path:?}.Please use --pid to specify the pid of the horust process you want to talk to.");
        }
        ret
    } else {
        pid.map(|p| format!("{p}.uds")).unwrap()
    };
    debug!("Socket filename: {socket_file_name}");
    Ok(sockets_folder_path.join(socket_file_name))
}

fn handle_status(socket_path: PathBuf) -> Result<()> {
    // `args` returns the arguments passed to the program
    let args: Vec<String> = env::args().map(|x| x.to_string()).collect();

    // Connect to socket
    let mut stream = match UnixStream::connect(&socket_path) {
        Err(_) => panic!("server is not running"),
        Ok(stream) => stream,
    };

    // Send message
    if let Err(_) = stream.write(b"hello") {
        panic!("couldn't send message")
    }
    Ok(())
}
