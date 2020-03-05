use clap::Clap;
use horust::Horust;
use std::path::PathBuf;

#[macro_use]
extern crate log;

#[derive(Clap, Debug)]
#[clap(author, about, version)]
/// Horust is a complete supervisor and init system, designed for running in containers.
struct Opts {
    #[clap(long, default_value = "/etc/horust/horust.toml")]
    /// Horust's config.
    config: String,
    #[clap(long)]
    /// Prints a service file with all the possible options
    sample_service: bool,
    #[clap(long, default_value = "/etc/horust/services")]
    /// Path to the directory containing the services
    services_path: PathBuf,
    #[clap(required = false, multiple = true, min_values = 0, last = true)]
    /// Specify a command to run instead of load services path. Useful if you just want to use the reaping capability. Preceed it with --.
    command: Vec<String>,
}

fn main() -> Result<(), horust::HorustError> {
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
    let mut horust = if !opts.command.is_empty() {
        debug!("Going to run command: {:?}", opts.command);

        Horust::from_command(
            opts.command
                .into_iter()
                .fold(String::new(), |acc, w| format!("{} {}", acc, w)),
        )
    } else {
        debug!(
            "Going to load services from directory: {}",
            opts.services_path.display()
        );
        Horust::from_services_dir(&opts.services_path)?
    };

    horust.run();
    Ok(())
}
