mod horust;

use crate::horust::{Horust, Service};
use std::path::PathBuf;
use structopt::StructOpt;

#[macro_use]
extern crate log;

#[derive(StructOpt, Debug)]
#[structopt(version = "0.1", author = "Federico Ponzi", name = "horust")]
/// Horust is a complete supervisor and init system, designed for running in containers.
struct Opts {
    #[structopt(short = "c", long = "config", default_value = "default.conf")]
    config: String,
    #[structopt(long = "sample-service")]
    sample_service: bool,
    #[structopt(long = "services-path", default_value = "/etc/horust/services")]
    services_path: PathBuf,
}

fn main() -> Result<(), horust::HorustError> {
    // Set up logging.
    let env = env_logger::Env::new()
        .filter("HORUST_LOG")
        .write_style("HORUST_LOG_STYLE");
    env_logger::init_from_env(env);

    //chdir("/").expect("Error: chdir()");

    let opts = Opts::from_args();
    if opts.sample_service {
        println!("{}", Service::get_sample_service());
        return Ok(());
    }
    Horust::from_services_dir(&opts.services_path)?.run()
}
