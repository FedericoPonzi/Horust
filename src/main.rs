mod error;
mod formats;
mod horust;
use crate::horust::Horust;
use nix::unistd::chdir;
use structopt::StructOpt;
#[macro_use]
extern crate log;

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
    #[structopt(long = "sample-service")]
    sample_service: bool,
}

const SAMPLE: &str = r#"name = "my-cool-service"
command = "/home/isaacisback/dev/rust/horust/examples/services/first.sh"
working-directory = "/tmp/"
restart = "never"
start-delay = "2s"
#restart-backoff = "10s"#;

fn main() -> Result<(), error::HorustError> {
    // Set up logging.
    let env = env_logger::Env::new()
        .filter("HORUST_LOG")
        .write_style("HORUST_LOG_STYLE");
    env_logger::init_from_env(env);

    chdir("/");

    let opt = Opts::from_args();
    if opt.sample_service {
        println!("{}", SAMPLE);
        return Ok(());
    }
    let path = "/home/isaacisback/dev/rust/horust/examples/services";
    Horust::from_services_dir(path)?.run()
}
