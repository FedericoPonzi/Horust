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
/// Horust is a complete supervisor and init system, designed for running in containers.
struct Opts {
    #[structopt(short = "c", long = "config", default_value = "default.conf")]
    config: String,
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

    //chdir("/").expect("Error: chdir()");

    let opt = Opts::from_args();
    if opt.sample_service {
        println!("{}", SAMPLE);
        return Ok(());
    }
    let path = "/home/isaacisback/dev/rust/horust/examples/services/2/bigger";
    Horust::from_services_dir(path)?.run()
}
