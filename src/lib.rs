//! # systemd-wake
//! 
//! This is a utility library that uses systemd-run under the hood to schedule any [`Command`] to
//! run at some future time. Allows for tasks to be scheduled and cancelled using custom systemd
//! unit names as handles. Note that there are no guarantees about naming collisions from other
//! programs. Be smart about choosing names.
//!
//! Requires the systemd-wake binary to be installed in order to work. Remember to install with
//! `cargo install systemd-wake`.
//!
//! Use [`register()`] to schedule a command with systemd-run to wake at specificed time.
//!
//! Use [`deregister()`] to cancel timer.
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
//! let unit_name = UnitName::new("my-special-unit-name-123").unwrap();
//!
//! // register future beep
//! systemd_wake::register(waketime,unit_name,command).unwrap();
//!
//! // check future beep
//! systemd_wake::query_registration(unit_name).unwrap();
//!
//! // cancel future beep
//! systemd_wake::deregister(unit_name).unwrap();
//! ```

#![deny(missing_docs)]

/// Command serialization.
pub mod command;
use command::{CommandConfig,CommandConfigError};

use std::fmt::{Display,Formatter};
use std::process::{Command,Output};

use chrono::NaiveDateTime;
use thiserror::Error;
#[allow(unused_imports)]
use tracing::{info,debug,warn,error,trace,Level};

/// Wrapper struct for the name given to the systemd timer unit.
#[derive(Copy,Clone)]
pub struct UnitName<'a> {
    name: &'a str,
}

impl<'a> UnitName<'a> {
    /// Creates new TimerName and verifies that unit name meets constraints of being only
    /// non-whitespace ASCII.
    pub fn new(name: &'a str) -> Result<Self,UnitNameError> {
        if !name.is_ascii() {
            return Err(UnitNameError::NotAscii);
        }
        if name.contains(char::is_whitespace) {
            return Err(UnitNameError::ContainsWhitespace);
        }
        Ok(Self { name })
    }
}

impl AsRef<str> for UnitName<'_> {
    fn as_ref(&self) -> &str {
        self.name
    }
}

impl Display for UnitName<'_> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        self.name.fmt(f)
    }
}

/// Error struct for creating [`UnitName`].
#[derive(Error,Debug)]
#[allow(missing_docs)]
pub enum UnitNameError {
    #[error("UnitName must be ASCII")]
    NotAscii,
    #[error("UnitName cannot conatin whitespace")]
    ContainsWhitespace,
}

/// Error struct for registration.
#[derive(Error,Debug)]
#[allow(missing_docs)]
pub enum RegistrationError {
    #[error("error querying timer status")]
    Query(#[from] QueryError),
    #[error("unit name is already in use")]
    Duplicate,
    #[error("error with registration command")]
    Command(#[from] CommandError),
}

/// Calls systemd-run to register command to wake at specified time using provided name.
pub fn register(event_time: NaiveDateTime, unit_name: UnitName, command: Command) -> Result<(),RegistrationError> {
    debug!("registering timer");

    if check_loaded(unit_name)? {
        return Err(RegistrationError::Duplicate);
    }

    let unit_name = format!("--unit={}",unit_name);

    let on_calendar = event_time.format("--on-calendar=%F %T").to_string();
    debug!("timer set for {}",on_calendar);

    let encoded_command = CommandConfig::encode(command).unwrap();

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
pub fn deregister(unit_name: UnitName) -> Result<(),CommandError> {
    debug!("deregistering timer");

    let unit_name = {
        let mut name = unit_name.to_string();
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

fn extract_property(unit_name: UnitName, property: &str) -> Result<String,QueryError> {
    let unit_name = {
        let mut name = unit_name.to_string();
        name.push_str(".timer");
        name
    };

    let mut systemd_command = Command::new("systemctl");
    systemd_command
        .arg("--user")
        .arg("show")
        .arg(unit_name)
        .arg(format!("--property={}",property));

    let output = run_command(systemd_command)?;

    match String::from_utf8(output.stdout) {
        Ok(string) => {
            if let Some(value) = string.strip_prefix(&format!("{}=",property)) {
                return Ok(value.trim_end().to_owned())
            } else {
                return Err(QueryError::ParseError);
            }
        },
        Err(_) => return Err(QueryError::ParseError),
    }
}

fn check_loaded(unit_name: UnitName) -> Result<bool,QueryError> {
    Ok(extract_property(unit_name,"LoadState")? == "loaded")
}

/// Returns registered command and wake up time for unit if it exists.
pub fn query_registration(unit_name: UnitName) -> Result<(Command,NaiveDateTime),QueryError> {
    debug!("querying registration");
    // look for:
    // LoadState
    // Description
    // TimersCalendar

    if !check_loaded(unit_name)? {
        return Err(QueryError::NotLoaded);
    }

    let desc = extract_property(unit_name, "Description")?;
    let command = if let Some(splits) = desc.split_once(" ") {
        CommandConfig::decode(splits.1)?
    } else {
        return Err(QueryError::ParseError);
    };

    let calendar = extract_property(unit_name, "TimersCalendar")?;
    let datetime_str = calendar
        .split_once("OnCalendar=").ok_or(QueryError::ParseError)?.1
        .split_once(" ;").ok_or(QueryError::ParseError)?.0;

    let datetime = match chrono::NaiveDateTime::parse_from_str(&datetime_str,"%Y-%m-%d %H:%M:%S") {
        Ok(x) => x,
        Err(_) => return Err(QueryError::ParseError),
    };

    Ok((command,datetime))

}

/// Error struct for querying task registration.
#[derive(Error,Debug)]
pub enum QueryError {
    /// Error sending command to systemd
    #[error("systemd command error")]
    Command(#[from] CommandError),
    /// Provided unit name is not loaded
    #[error("unit with provided name not loaded")]
    NotLoaded,
    /// Error parsing systemd output
    #[error("error parsing systemd output")]
    ParseError,
    /// Error decoding command
    #[error("error decoding command")]
    DecodeError(#[from] CommandConfigError),
}

/// Error struct for running a command. Wraps running with a non-success exit status as an error variant.
#[derive(Error,Debug)]
pub enum CommandError {
    /// Error running the command
    #[error("error running command")]
    RunCommand(#[from] std::io::Error),
    /// Command ran, but exited with failure status
    #[error("command exited with failure status")]
    CommandFailed(Output),
}

/// Helper function for running commands.
pub fn run_command(mut command: Command) -> Result<Output,CommandError> {
    match command.output() {
        Ok(output) => {
            if output.status.success() {
                Ok(output)
            } else {
                Err(CommandError::CommandFailed(output))
            }
        },
        Err(e) => {
            Err(CommandError::RunCommand(e))
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
        let unit_name = UnitName::new("my-special-unit-name-123").unwrap();

        // register future beep
        register(waketime,unit_name,command).unwrap();

        // check future beep
        let (_command, _datetime) = query_registration(unit_name).unwrap();

        // cancel future beep
        deregister(unit_name).unwrap();
    }
}
