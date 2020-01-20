mod error;
mod formats;
mod horust;
mod runtime;
use crate::horust::Horust;
use std::time::Duration;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(version = "0.1", author = "Federico Ponzi", name = "horust")]
/// Horust is an supervisor system designed for containers.
struct Opts {
    /// Sets a custom config file. Could have been an Option<T> with no default too
    #[structopt(short = "c", long = "config", default_value = "default.conf")]
    config: String,
    #[structopt(short, long, parse(from_occurrences))]
    /// A level of verbosity, and can be used multiple times
    verbose: i32,
}

fn main() -> Result<(), error::HorustError> {
    /*
    if (getpid() != 1) {
        std::process::exit(1);
    }
    chdir("/");
    */
    let opt = Opts::from_args();
    println!("Opts: {:#?}", opt);
    let path = "/home/isaacisback/dev/rust/horust/examples/services";
    Horust::from_services_dir(path)?.run()
}
