//! `floti-fake-gui`: a program with no console, which is what a GUI app, or
//! a tool a CLI starts with its window hidden, is on Windows. It is built for
//! the `windows` subsystem, so it attaches to no console and gets none of a
//! console's events; macOS and Linux ignore the subsystem and run it as any
//! other program. The fake agent starts one for `--fake-gui-child`.
//!
//! It does nothing for `--fake-sleep=<ms>` milliseconds (ten minutes by
//! default), then exits 0, or exits 0 sooner once the test process that owns
//! it is gone, as the fake agent does (`owner.rs`).
//!
//! With `--console-of=<pid>` it only says whether process `pid` has a
//! console, hidden or not, by asking Windows to attach it to that console
//! (`AttachConsole`), which only a program with no console of its own may
//! ask: it exits 0 when `pid` has one, 1 when it has none, and 2, with why
//! on standard error, when that could not be told, as on macOS and Linux,
//! which have no consoles. The tests that a host Floti starts has no console
//! ask it.

#![windows_subsystem = "windows"]

mod owner;

use std::path::PathBuf;
use std::time::Duration;

use floti_host::platform::process_tree::{Native, ProcessTree};
use floti_protocol::Knowable;

const DEFAULT_MS: u64 = 600_000;

/// The fake agent's record folder, which it hands on to its children.
const LOG_VAR: &str = "FLOTI_FAKE_AGENT_LOG";

/// Exits 0 when process `pid` has a console, 1 when it has none, 2 when
/// that could not be told.
#[cfg(windows)]
fn console_of(pid: u32) -> i32 {
    use windows_sys::Win32::Foundation::{ERROR_INVALID_HANDLE, GetLastError};
    use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole};
    // SAFETY: plain calls on a process id; a console attached to is let go
    // of at once.
    if unsafe { AttachConsole(pid) } != 0 {
        unsafe { FreeConsole() };
        return 0;
    }
    // SAFETY: reads the calling thread's last error.
    match unsafe { GetLastError() } {
        ERROR_INVALID_HANDLE => 1,
        other => {
            eprintln!(
                "could not tell whether process {pid} has a console: {}",
                std::io::Error::from_raw_os_error(other.cast_signed())
            );
            2
        }
    }
}

#[cfg(not(windows))]
fn console_of(_pid: u32) -> i32 {
    eprintln!("only Windows gives a process a console to look for");
    2
}

fn main() {
    if let Some(pid) = std::env::args()
        .skip(1)
        .find_map(|arg| arg.strip_prefix("--console-of=")?.parse().ok())
    {
        std::process::exit(console_of(pid));
    }
    let watched = std::env::var_os(LOG_VAR)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    if let Some(folder) = watched {
        owner::exit_when_owner_is_gone(&folder, |pid| {
            Native.is_alive(pid) != Knowable::Known(false)
        });
    }
    let ms = std::env::args()
        .skip(1)
        .find_map(|arg| arg.strip_prefix("--fake-sleep=")?.parse().ok())
        .unwrap_or(DEFAULT_MS);
    std::thread::sleep(Duration::from_millis(ms));
}
