
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
    pub fn encode(command: Command) -> Result<String,CommandConfigError> {
        let config: CommandConfig = command.into();
        let json = serde_json::to_string(&config)
            .map_err(|e| CommandConfigError { kind: CommandConfigErrorKind::Serialization(e) })?;
        Ok(hex::encode(json))
    }
    
    pub fn decode(hexcode: impl AsRef<[u8]>) -> Result<Command,CommandConfigError> {
        let bytes = hex::decode(hexcode).map_err(|e| CommandConfigError { kind: CommandConfigErrorKind::HexDecode(e) })?;
        let json = String::from_utf8(bytes).map_err(|e| CommandConfigError { kind: CommandConfigErrorKind::Utf8(e) })?;
        let config: CommandConfig = serde_json::from_str(&json).map_err(|e| CommandConfigError { kind: CommandConfigErrorKind::Deserialization(e) })?;
        Ok(config.into())
    }
}

/// Error type for CommandConfig
#[derive(Debug)]
pub struct CommandConfigError {
    /// The kind of error that occurred
    pub kind: CommandConfigErrorKind,
}

/// Error kind for [`CommandConfigError`]
#[allow(missing_docs)]
#[derive(Debug)]
pub enum CommandConfigErrorKind {
    Serialization(serde_json::Error),
    Deserialization(serde_json::Error),
    HexDecode(hex::FromHexError),
    Utf8(std::string::FromUtf8Error),
}

impl Display for CommandConfigError {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f,"Failed to decode/encode command")
    }
}

impl std::error::Error for CommandConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            CommandConfigErrorKind::Serialization(e) => Some(e),
            CommandConfigErrorKind::Deserialization(e) => Some(e),
            CommandConfigErrorKind::HexDecode(e) => Some(e),
            CommandConfigErrorKind::Utf8(e) => Some(e),
        }
    }
}
