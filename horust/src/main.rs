use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use horust::horust::ExitStatus;
use horust::Horust;
use log::{error, info};
use nix::unistd::getpid;

#[derive(clap::Parser, Debug)]
#[clap(author, about)]
/// Horust is a complete supervisor and init system, designed for running in containers.
struct Opts {
    #[arg(long, default_value = "/etc/horust/horust.toml")]
    /// Horust's path to config.
    config_path: PathBuf,

    #[clap(long)]
    /// Exits with an unsuccessful exit code if any process is in FinishedFailed state
    pub unsuccessful_exit_finished_failed: bool,

    #[arg(long)]
    /// Print a sample service file with all the possible options
    sample_service: bool,

    #[arg(long = "services-path", default_value = "/etc/horust/services")]
    /// Path to service file or a directory containing services to run. You can provide more than one argument to load multiple directories / services.
    services_paths: Vec<PathBuf>,

    #[arg(required = false, long, default_value = "/var/run/horust")]
    /// Path to the folder that contains the Unix Domain Socket, used to communicate with horustctl
    uds_folder_path: PathBuf,

    #[arg(required = false, last = true)]
    /// Specify a command to run instead of load services path. Useful if you just want to use the reaping capability. Prefix your command with --
    command: Vec<String>,
}

fn main() -> Result<()> {
    // Set up logging.
    let env = env_logger::Env::new()
        .filter("HORUST_LOG")
        .write_style("HORUST_LOG_STYLE");
    env_logger::init_from_env(env);

    let opts = Opts::parse();

    if opts.sample_service {
        println!("{}", horust::get_sample_service());
        return Ok(());
    }

    if !opts.uds_folder_path.exists() {
        std::fs::create_dir_all(&opts.uds_folder_path)?;
    }

    if !opts.uds_folder_path.is_dir() {
        panic!(
            "'{:?}' is not a directory. Use --uds-folder-path to select a different folder.",
            opts.uds_folder_path
        );
    }
    let uds_path = horust_commands_lib::get_path(&opts.uds_folder_path, getpid().into());

    let mut horust = if opts.command.is_empty() {
        info!(
            "Loading services from {}",
            display_directories(&opts.services_paths)
        );
        Horust::from_services_dirs(&opts.services_paths, uds_path).with_context(|| {
            format!(
                "Failed loading services from {}",
                display_directories(&opts.services_paths)
            )
        })?
    } else {
        info!("Running command: {:?}", opts.command);
        Horust::from_command(opts.command.join(" "), uds_path)
    };

    if let ExitStatus::SomeServiceFailed = horust.run() {
        if opts.unsuccessful_exit_finished_failed {
            error!("Some processes have failed.");
            std::process::exit(101);
        }
    }
    Ok(())
}

fn display_directories(dirs: &[PathBuf]) -> String {
    match dirs.len() {
        1 => format!("directory: {}", dirs.first().unwrap().display()),
        _ => format!(
            "directories:\n{}",
            dirs.iter()
                .map(|d| format!("* {}", d.display()))
                .collect::<Vec<String>>()
                .join("\n"),
        ),
    }
}
