//! The terminal this process runs in: the size of the one it writes to
//! ([`TerminalSize`]), and whether it may read an answer from the one it
//! reads ([`Foreground`]).
//!
//! Only the fake agent asks the size, to say what size its pty has, so a test
//! can see that a resize reached a program's terminal. `floti setup` asks
//! whether it is in the foreground before it asks its question.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::Native;
#[cfg(target_os = "macos")]
pub use macos::Native;
#[cfg(target_os = "windows")]
pub use windows::Native;

use std::io;

/// Whether this process may read from the terminal standard input is
/// without being stopped for it.
pub trait Foreground {
    /// Whether this process is in the foreground of the terminal standard
    /// input is. A job put in the background (`curl ... | sh &`) is not, and
    /// is stopped when it reads; with no terminal, or no job control, the
    /// answer is about that alone.
    fn owns_standard_input(&self) -> bool;
}

/// The OS's way to read the size of the terminal standard output is.
pub trait TerminalSize {
    /// The columns and rows of the terminal standard output is; an error
    /// when standard output is no terminal.
    fn of_standard_output(&self) -> io::Result<(u16, u16)>;
}
