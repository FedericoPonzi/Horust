use anyhow::{Context, Result};
use horust::horust::ExitStatus;
use horust::horust::HorustConfig;
use horust::Horust;
use itertools::Itertools;
use std::path::PathBuf;
use structopt::StructOpt;

#[macro_use]
extern crate log;

#[derive(StructOpt, Debug)]
#[structopt(author, about)]
/// Horust is a complete supervisor and init system, designed for running in containers.
struct Opts {
    #[structopt(long, default_value = "/etc/horust/horust.toml")]
    /// Horust's path to config.
    config_path: PathBuf,

    #[structopt(flatten)]
    horust_config: HorustConfig,

    #[structopt(long)]
    /// Prints a sample service file with all the possible options
    sample_service: bool,

    #[structopt(long = "services-path", default_value = "/etc/horust/services")]
    /// Path to the directory containing the services. User can provide more than one argument to load multiple directories.
    services_paths: Vec<PathBuf>,

    #[structopt(required = false, multiple = true, min_values = 0, last = true)]
    /// Specify a command to run instead of load services path. Useful if you just want to use the reaping capability. Prefix your command with --
    command: Vec<String>,
}

fn main() -> Result<()> {
    // Set up logging.
    let env = env_logger::Env::new()
        .filter("HORUST_LOG")
        .write_style("HORUST_LOG_STYLE");
    env_logger::init_from_env(env);

    let opts = Opts::from_args();

    if opts.sample_service {
        println!("{}", horust::get_sample_service());
        return Ok(());
    }

    let config = HorustConfig::load_and_merge(&opts.horust_config, &opts.config_path)
        .with_context(|| {
            format!(
                "Failed loading configuration: {}",
                &opts.config_path.display()
            )
        })?;

    let mut horust = if !opts.command.is_empty() {
        info!("Running command: {:?}", opts.command);

        Horust::from_command(
            opts.command
                .into_iter()
                .fold(String::new(), |acc, w| format!("{} {}", acc, w)),
        )
    } else {
        info!(
            "Loading services from {}",
            display_directories(&opts.services_paths)
        );
        Horust::from_services_dirs(&opts.services_paths).with_context(|| {
            format!(
                "Failed loading services from {}",
                display_directories(&opts.services_paths)
            )
        })?
    };

    if let ExitStatus::SomeServiceFailed = horust.run() {
        if config.unsuccessful_exit_finished_failed {
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
            dirs.iter().map(|d| format!("* {}", d.display())).join("\n")
        ),
    }
}
