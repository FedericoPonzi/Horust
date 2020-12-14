use horust::horust::ExitStatus;
use horust::horust::HorustConfig;
use horust::Horust;
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

    #[structopt(short = "v", parse(from_occurrences))]
    /// Allows to tune Horust's verbosity
    log_verbosity: u32,

    #[structopt(long)]
    /// Prints a sample service file with all the possible options
    sample_service: bool,

    #[structopt(long, default_value = "/etc/horust/services")]
    /// Path to the directory containing the services
    services_path: PathBuf,

    #[structopt(required = false, multiple = true, min_values = 0, last = true)]
    /// Specify a command to run instead of load services path. Useful if you just want to use the reaping capability. Prefix your command with --
    command: Vec<String>,
}
fn log_verbosity(val: u32) -> &'static str {
    match val {
        2 => "debug",
        3 => "trace",
        _ => "info",
    }
}

fn main() -> Result<(), horust::HorustError> {
    let opts = Opts::from_args();

    // Set up logging.
    let env = env_logger::Env::new()
        .filter_or("HORUST_LOG", log_verbosity(opts.log_verbosity))
        .write_style("HORUST_LOG_STYLE");
    env_logger::init_from_env(env);

    if opts.sample_service {
        println!("{}", horust::get_sample_service());
        return Ok(());
    }

    let config = HorustConfig::load_and_merge(opts.horust_config, &opts.config_path)?;

    let mut horust = if !opts.command.is_empty() {
        debug!("Running command: {:?}", opts.command);

        Horust::from_command(
            opts.command
                .into_iter()
                .fold(String::new(), |acc, w| format!("{} {}", acc, w)),
        )
    } else {
        debug!(
            "Loading services from directory: {}",
            opts.services_path.display()
        );
        Horust::from_services_dir(&opts.services_path)?
    };

    if let ExitStatus::SomeServiceFailed = horust.run() {
        if config.unsuccessful_exit_finished_failed {
            std::process::exit(101);
        }
    }
    Ok(())
}
