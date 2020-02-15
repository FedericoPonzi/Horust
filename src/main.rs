use crate::horust::{Horust, Service};
use std::path::PathBuf;
use structopt::StructOpt;

mod horust;

#[macro_use]
extern crate log;

#[derive(StructOpt, Debug)]
#[structopt(version = "0.1", author = "Federico Ponzi", name = "horust")]
/// Horust is a complete supervisor and init system, designed for running in containers.
struct Opts {
    #[structopt(short, long, default_value = "/etc/horust/horust.toml")]
    /// Horust's config.
    config: String,
    #[structopt(long)]
    /// Prints a service file with all the possible options
    sample_service: bool,
    #[structopt(long, default_value = "/etc/horust/services")]
    /// Path to the directory containing the services
    services_path: PathBuf,
    #[structopt()]
    /// Specify a command to run instead of load services path. Useful if you just want to use the reaping capability.
    command: Option<String>,
}

fn main() -> Result<(), horust::HorustError> {
    // Set up logging.
    let env = env_logger::Env::new()
        .filter("HORUST_LOG")
        .write_style("HORUST_LOG_STYLE");
    env_logger::init_from_env(env);

    let opts = Opts::from_args();

    if opts.sample_service {
        println!("{}", Service::get_sample_service());
        return Ok(());
    }
    if let Some(command) = opts.command {
        let service = Service::from_command(command);
        Horust::from_service(service).run()
    } else {
        Horust::from_services_dir(&opts.services_path)?.run()
    }
}
