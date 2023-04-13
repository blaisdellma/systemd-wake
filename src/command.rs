
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use serde::{Serialize,Deserialize};
#[allow(unused_imports)]
use tracing::{info,debug,warn,error,trace,Level};
use thiserror::Error;

/// Non-runnable version of [`Command`] used for serialization.
#[derive(Serialize,Deserialize)]
pub struct CommandConfig {
    program: OsString,
    dir: Option<PathBuf>,
    env_vars: Vec<(OsString,Option<OsString>)>,
    args: Vec<OsString>,
}

impl From<Command> for CommandConfig {
    fn from(command: Command) -> Self {
        let program = command.get_program().into();
        let dir = command.get_current_dir().map(|path| path.to_path_buf());
        let env_vars = command.get_envs().map(|(key, value)| {
            (key.to_os_string(), value.map(|value| value.to_os_string()))
        }).collect();
        let args = command.get_args().map(|value| value.to_os_string()).collect();
        CommandConfig {
            program,
            dir,
            env_vars,
            args,
        }
    }
}

impl From<CommandConfig> for Command {
    fn from(config: CommandConfig) -> Self {
        let mut command = Command::new(config.program);
        command.args(config.args);
        for (key, value) in config.env_vars {
            match value {
                Some(value) => {
                    command.env(key,value);
                },
                None => {
                    command.env_remove(key);
                }
            }
        }
        match config.dir {
            Some(dir) => {
                command.current_dir(dir);
            }
            None => {},
        };
        command
    }
}

#[allow(missing_docs)]
impl CommandConfig {
    pub fn encode(command: Command) -> Result<String,CommandConfigError> {
        let config: CommandConfig = command.into();
        let json = serde_json::to_string(&config)?;
        Ok(hex::encode(json))
    }
    
    pub fn decode(hexcode: impl AsRef<[u8]>) -> Result<Command,CommandConfigError> {
        let bytes = hex::decode(hexcode)?;
        let json = String::from_utf8(bytes)?;
        let config: CommandConfig = serde_json::from_str(&json)?;
        Ok(config.into())
    }
}


/// Error type for CommandConfig.
#[derive(Error,Debug)]
#[allow(missing_docs)]
pub enum CommandConfigError {
    #[error("json (de)serialization error")]
    SerdeJson(#[from] serde_json::Error),
    #[error("hex (de/en)coding error")]
    Hex(#[from] hex::FromHexError),
    #[error("utf8 parsing error")]
    Utf8(#[from] std::string::FromUtf8Error),
}
