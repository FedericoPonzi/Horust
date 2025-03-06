use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::Context;
#[cfg(feature = "http-healthcheck")]
use reqwest::blocking::Client;

use crate::horust::formats::Healthiness;
use crate::horust::supervisor::find_program;

const FILE_CHECK: FilePathCheck = FilePathCheck {};
const HTTP_CHECK: HttpCheck = HttpCheck {};
const COMMAND_CHECK: CommandCheck = CommandCheck {};
const CHECKS: [&dyn Check; 3] = [&FILE_CHECK, &HTTP_CHECK, &COMMAND_CHECK];

type ParsedCommands = Mutex<HashMap<String, Vec<String>>>;
static PARSED_COMMANDS: OnceLock<ParsedCommands> = OnceLock::new();

fn get_parsed_commands() -> &'static ParsedCommands {
    PARSED_COMMANDS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn get_checks() -> [&'static dyn Check; 3] {
    CHECKS
}

pub(crate) trait Check {
    fn run(&self, healthiness: &Healthiness) -> bool;
    fn prepare(&self, _healtiness: &Healthiness) -> Result<(), std::io::Error> {
        Ok(())
    }
}

/// HTTP based healthcheck: will send an head request with 1 second timeout, and the test will be
/// considered failed if the response is anything other than `200`.
pub(crate) struct HttpCheck;

static HTTP_REQUEST_TIMEOUT: u64 = 1;

impl Check for HttpCheck {
    fn run(&self, healthiness: &Healthiness) -> bool {
        healthiness
            .http_endpoint.as_ref()
            .map(|endpoint| {
                if cfg!(not(feature = "http-healthcheck")) {
                    error!("There is an http based healthcheck, but horust was built without the http-healthcheck feature (thus it will never pass these checks).");
                    return false;
                }
                #[cfg(feature = "http-healthcheck")]
                    {
                        let client = Client::builder()
                            .timeout(Duration::from_secs(HTTP_REQUEST_TIMEOUT))
                            .build().expect("Http client");
                        let resp: Result<reqwest::blocking::Response, reqwest::Error> = client.head(endpoint).send();
                        resp.map(|resp| resp.status().is_success()).unwrap_or(false)
                    }
            })
            .unwrap_or(true)
    }
}

pub(crate) struct FilePathCheck;

impl Check for FilePathCheck {
    fn run(&self, healthiness: &Healthiness) -> bool {
        healthiness
            .file_path
            .as_ref()
            .map(|file_path| file_path.exists())
            .unwrap_or(true)
    }
    fn prepare(&self, healthiness: &Healthiness) -> Result<(), std::io::Error> {
        //TODO: check if user has permissions to remove the file.
        healthiness
            .file_path
            .as_ref()
            .filter(|file| file.exists())
            .map(std::fs::remove_file)
            .unwrap_or(Ok(()))
    }
}

pub(crate) struct CommandCheck {}

impl CommandCheck {
    fn prepare_cmd(&self, cmd: &str) -> anyhow::Result<()> {
        let mut chunks = shlex::split(cmd).context(format!("Failed to split command: {}", cmd))?;
        let program = chunks
            .first()
            .context(format!("Failed to get program from command: {}", cmd))?;
        let path = if program.contains('/') {
            program.to_string()
        } else {
            find_program(program)?
        };
        chunks[0] = path;
        get_parsed_commands()
            .lock()
            .unwrap()
            .insert(cmd.to_string(), chunks);
        Ok(())
    }
}

impl Check for CommandCheck {
    fn run(&self, healthiness: &Healthiness) -> bool {
        healthiness
            .command
            .as_ref()
            .map(|command| {
                let parsed_command = get_parsed_commands().lock().unwrap().get(command).cloned();
                parsed_command
                    .map(|cmds| {
                        let output = std::process::Command::new(&cmds[0])
                            .args(&cmds[1..])
                            .output()
                            .expect("Failed to execute command");
                        output.status.success()
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(true)
    }
    fn prepare(&self, healtiness: &Healthiness) -> Result<(), std::io::Error> {
        healtiness
            .command
            .as_ref()
            .map(|command| {
                self.prepare_cmd(command)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                Ok(())
            })
            .unwrap_or(Ok(()))
    }
}
