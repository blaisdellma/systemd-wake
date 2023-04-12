//! # systemd-wake
//! 
//! This is a utility library that uses systemd-run under the hood to schedule any [`Command`] to
//! run at some future time. Allows for tasks to be scheduled and cancelled using custom systemd
//! unit names as handles. Note that there are no guarantees about naming collisions from other
//! programs. Be smart about choosing names.
//!
//! Requires the systemd-wake binary to be installed in order to work. Remember to install with
//! `cargo install systemd-wake`
//!
//! Use [`register()`] to schedule a command with systemd-run to wake at specificed time
//!
//! Use [`deregister()`] to cancel timer
//!
//! ### Example
//! ```
//! use systemd_wake::*;
//!
//! // one minute in the future
//! let waketime = chrono::Local::now() + chrono::Duration::minutes(1);
//!
//! // schedule a short beep
//! let mut command = std::process::Command::new("play");
//! command.args(vec!["-q","-n","synth","0.1","sin","880"]);
//!
//! // create unit handle
//! let timer_name = TimerName::new("my-special-unit-name-123").unwrap();
//!
//! // register future beep
//! systemd_wake::register(waketime,timer_name,command).unwrap();
//!
//! // cancel future beep
//! systemd_wake::deregister(timer_name).unwrap();
//! ```

#![deny(missing_docs)]

use std::ffi::OsString;
use std::fmt::{Display,Formatter};
use std::path::PathBuf;
use std::process::{Command,Output};

use chrono::{Local,NaiveDateTime};
use serde::{Serialize,Deserialize};
#[allow(unused_imports)]
use tracing::{info,debug,warn,error,trace,Level};

/// Wrapper struct for the name given to the systemd timer unit.
#[derive(Copy,Clone)]
pub struct TimerName<'a> {
    name: &'a str,
}

impl<'a> TimerName<'a> {
    /// Creates new TimerName and verifies that unit name meets constraints of being only
    /// non-whitespace ASCII.
    pub fn new(name: &'a str) -> Result<Self,TimerNameError> {
        if !name.is_ascii() {
            return Err(TimerNameError { kind: TimerNameErrorKind::NotAscii });
        }
        if name.contains(char::is_whitespace) {
            return Err(TimerNameError { kind: TimerNameErrorKind::ContainsWhitespace });
        }
        Ok(Self { name })
    }
}

impl AsRef<str> for TimerName<'_> {
    fn as_ref(&self) -> &str {
        self.name
    }
}

impl Display for TimerName<'_> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        self.name.fmt(f)
    }
}

/// Error struct for creating TimerName.
#[derive(Debug)]
pub struct TimerNameError {
    kind: TimerNameErrorKind,
}

/// Error kinds for [`TimerNameError`].
#[derive(Debug)]
#[allow(missing_docs)]
pub enum TimerNameErrorKind {
    NotAscii,
    ContainsWhitespace,
}

impl Display for TimerNameError {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self.kind {
            TimerNameErrorKind::NotAscii => write!(f,"TimerName must be ASCII"),
            TimerNameErrorKind::ContainsWhitespace => write!(f,"TimerName cannot contain whitespace"),
        }
    }
}

impl std::error::Error for TimerNameError {}

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

/// Calls systemd-run to register command to wake at specified time using provided name.
pub fn register(event_time: NaiveDateTime, timer_name: TimerName, command: Command) -> Result<(),CommandError> {
    debug!("registering timer");

    let unit_name = format!("--unit={}",timer_name);

    let on_calendar = event_time.format("--on-calendar=%F %T").to_string();
    debug!("timer set for {}",on_calendar);

    let encoded_command = CommandConfig::encode(command);

    let mut systemd_command = Command::new("systemd-run");
    systemd_command
        .arg("--user")
        .arg(unit_name)
        .arg(on_calendar)
        .arg("systemd-wake")
        .arg(encoded_command);

    debug!("running timer command: {:?}",systemd_command);
    run_command(systemd_command)?;
    Ok(())
}

/// Calls systemctl to deregister specified timer.
pub fn deregister(timer_name: TimerName) -> Result<(),CommandError> {
    debug!("deregistering timer");

    let unit_name = {
        let mut name = timer_name.to_string();
        name.push_str(".timer");
        name
    };

    let mut systemd_command = Command::new("systemctl");
    systemd_command
        .arg("--user")
        .arg("stop")
        .arg(unit_name);

    debug!("running stop timer command: {:?}",systemd_command);
    run_command(systemd_command)?;
    Ok(())
}

/// Error struct for running a command. Wraps running with a non-success exit status as an error variant.
#[derive(Debug)]
pub struct CommandError {
    /// The command that was run
    pub command: Command,
    /// The kind of error that occured
    pub kind: CommandErrorKind,
}

/// Error kinds for [`CommandError`].
#[derive(Debug)]
pub enum CommandErrorKind {
    /// Error running the command
    RunCommand(std::io::Error),
    /// Command ran, but exited with failure status
    CommandFailed(Output),
}

impl Display for CommandError {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f,"systemd-run command failed: {:?}", self.command)
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            CommandErrorKind::RunCommand(e) => Some(e),
            CommandErrorKind::CommandFailed(_) => None,
        }
    }
}

/// Helper function for running commands.
pub fn run_command(mut command: Command) -> Result<Output,CommandError> {
    match command.output() {
        Ok(output) => {
            if output.status.success() {
                Ok(output)
            } else {
                Err(CommandError {
                    command,
                    kind: CommandErrorKind::CommandFailed(output),
                })
            }
        },
        Err(e) => {
            Err(CommandError {
                command,
                kind: CommandErrorKind::RunCommand(e),
            })
        }
    }
}
