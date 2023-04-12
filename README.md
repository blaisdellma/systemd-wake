# systemd-wake
[![crates.io](https://img.shields.io/crates/v/systemd-wake.svg)](https://crates.io/crates/systemd-wake)
[<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-systemd%2D-wake-f2049b?labelColor=555555&logo=docs.rs" height=20>](https://docs.rs/systemd-wake)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

This is a utility library using [`systemd-run`](https://www.freedesktop.org/software/systemd/man/systemd-run.html) to schedule wake ups for future tasks.

Custom systemd unit names are used as handles for the scheduled tasks. Note that there are no guarantees about naming collisions from other programs. Be smart about choosing names.

While any task constructable as a `std::process::Command` is allowed, this was created with the intent of allowing a perpetual, but only periodically active, task to dynamically schedule itself using systemd.

For example if the program knows it needs to do something so many hours from now, it can register itself with systemd-wake and then exit, freeing up resources in the meantime, before systemd wakes it up at the scheduled time. Then the program can do the work it needs to do before scheduling its next wake up time and exiting again.

Cron jobs are best for tasks with predicable scheduling. This is meant to fill the gap for tasks with dynamic schedules known only at run time.

The default precision for systemd-run is only 1 minute, so this is not suitable for tasks requiring small time precision scheduling.

# Install

Install with cargo:
```
cargo install systemd-wake
```
Now add to your Rust project:
```
cargo add systemd-wake
```

### NOTE:
The systemd-wake binary is required as it is used as an intermediary between the scheduled `std::process::Command` and systemd.

# Example
```
use systemd_wake::*;

// one minute in the future
let waketime = chrono::Local::now() + chrono::Duration::minutes(1);

// schedule a short beep
let mut command = std::process::Command::new("play");
command.args(vec!["-q","-n","synth","0.1","sin","880"]);

// create unit handle
let timer_name = TimerName::new("my-special-unit-name-123").unwrap();

// register future beep
systemd_wake::register(waketime,timer_name,command).unwrap();

// cancel future beep
systemd_wake::deregister(timer_name).unwrap();
```
### TODO
 - [ ] query status based on timer name
 - [ ] check for existing unit before scheduling with same name
 - [ ] return cancelled command on deregister
 - [ ] allow for rescheduling task without having to cancel and then reconstruct command
 - [ ] allow for the recovery of stdout, stderr, and exit status of scheduled command[^1]
 
 [^1]: I'm not sure what context this would even exist in? Maybe it would just get written out to a file?
