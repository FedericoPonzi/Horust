use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

// TODO: this should be an optional
// otherwise we wouldn't know if it was set to false on the commandline. Maybe. Because it's a flag.

#[derive(Debug, StructOpt, Serialize, Deserialize, Default)]
pub struct HorustConfig {
    #[structopt(long)]
    /// Exits with an unsuccessful exit code if any process is in FinishedFailed state
    pub unsuccessful_exit_finished_failed: bool,
}

impl HorustConfig {
    /// Load the config file, and handles the merge with the options defined in the cmdline.
    /// Cmdline defined values have precedence over config based values.
    pub fn load_and_merge(cmd_line: &HorustConfig, path: &Path) -> Result<Self> {
        let config_file: HorustConfig = if path.exists() {
            let content = std::fs::read_to_string(path)?;
            toml::from_str(&content)?
        } else {
            Default::default()
        };

        let unsuccessful_exit_finished_failed = cmd_line.unsuccessful_exit_finished_failed
            || config_file.unsuccessful_exit_finished_failed;

        Ok(HorustConfig {
            unsuccessful_exit_finished_failed,
        })
    }
}
