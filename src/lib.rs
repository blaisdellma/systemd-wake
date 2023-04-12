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
//! let waketime = chrono::Local::now().naive_local() + chrono::Duration::minutes(1);
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
//! // check future beep
//! systemd_wake::query_registration(timer_name).unwrap();
//!
//! // cancel future beep
//! systemd_wake::deregister(timer_name).unwrap();
//! ```

#![deny(missing_docs)]

use std::ffi::OsString;
use std::fmt::{Display,Formatter};
use std::path::PathBuf;
use std::process::{Command,Output};

use chrono::NaiveDateTime;
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

fn extract_property(timer_name: TimerName, property: &str) -> Result<String,QueryError> {
    let unit_name = {
        let mut name = timer_name.to_string();
        name.push_str(".timer");
        name
    };

    let mut systemd_command = Command::new("systemctl");
    systemd_command
        .arg("--user")
        .arg("show")
        .arg(unit_name)
        .arg(format!("--property={}",property));

    let output = run_command(systemd_command).map_err(|e| QueryError { kind: QueryErrorKind::Command(e) })?;

    match String::from_utf8(output.stdout) {
        Ok(string) => {
            if let Some(value) = string.strip_prefix(&format!("{}=",property)) {
                return Ok(value.trim_end().to_owned())
            } else {
                return Err(QueryError { kind: QueryErrorKind::ParseError });
            }
        },
        Err(_) => return Err(QueryError { kind: QueryErrorKind::ParseError }),
    }
}

/// Returns registered command if it exists
pub fn query_registration(timer_name: TimerName) -> Result<(Command,NaiveDateTime),QueryError> {
    debug!("querying registration");
    // look for:
    // LoadState
    // Description
    // TimersCalendar

    if extract_property(timer_name, "LoadState")? != "loaded" {
        return Err(QueryError { kind: QueryErrorKind::NotLoaded });
    }

    let desc = extract_property(timer_name, "Description")?;
    let command = if let Some(splits) = desc.split_once(" ") {
        CommandConfig::decode(splits.1)
    } else {
        return Err(QueryError { kind: QueryErrorKind::ParseError });
    };

    let calendar = extract_property(timer_name, "TimersCalendar")?;
    let datetime_str = calendar
        .split_once("OnCalendar=").ok_or(QueryError { kind: QueryErrorKind::ParseError })?.1
        .split_once(" ;").ok_or(QueryError { kind: QueryErrorKind::ParseError })?.0;

    let datetime = match chrono::NaiveDateTime::parse_from_str(&datetime_str,"%Y-%m-%d %H:%M:%S") {
        Ok(x) => x,
        Err(_) => return Err(QueryError { kind: QueryErrorKind::ParseError }),
    };

    Ok((command,datetime))

}

/// Error struct for querying task registration
#[derive(Debug)]
pub struct QueryError {
    /// The kind of error that occured
    pub kind: QueryErrorKind,
}

#[derive(Debug)]
/// Error kinds for [`QueryError`]
pub enum QueryErrorKind {
    /// Error sending command to systemd
    Command(CommandError),
    /// Provided unit name is not loaded
    NotLoaded,
    /// Error parsing systemd output
    ParseError,
}

impl Display for QueryError {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f,"failed to query task registration")
    }
}

impl std::error::Error for QueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            QueryErrorKind::Command(e) => Some(e),
            QueryErrorKind::NotLoaded => None,
            QueryErrorKind::ParseError => None,
        }
    }
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_beep() {
        // one minute in the future
        let waketime = chrono::Local::now().naive_local() + chrono::Duration::minutes(1);

        // schedule a short beep
        let mut command = std::process::Command::new("play");
        command.args(vec!["-q","-n","synth","0.1","sin","880"]);

        // create unit handle
        let timer_name = TimerName::new("my-special-unit-name-123").unwrap();

        // register future beep
        register(waketime,timer_name,command).unwrap();

        // check future beep
        let (_command, _datetime) = query_registration(timer_name).unwrap();

        // cancel future beep
        deregister(timer_name).unwrap();
    }
}
