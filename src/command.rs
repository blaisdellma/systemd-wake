
use std::ffi::OsString;
use std::fmt::{Display,Formatter};
use std::path::PathBuf;
use std::process::Command;

use serde::{Serialize,Deserialize};
#[allow(unused_imports)]
use tracing::{info,debug,warn,error,trace,Level};

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
    pub fn encode(command: Command) -> String {
        let config: CommandConfig = command.into();
        hex::encode(serde_json::to_string(&config).unwrap())
    }
    
    pub fn decode(hexcode: impl AsRef<[u8]>) -> Command {
        let config: CommandConfig = serde_json::from_str(&String::from_utf8(hex::decode(hexcode).unwrap()).unwrap()).unwrap();
        config.into()
    }
}
