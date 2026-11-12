//! The terminal on macOS and Linux: the window size the pty keeps, read
//! with `TIOCGWINSZ` on standard output, and the terminal's foreground
//! process group, read with `tcgetpgrp` on standard input.

use std::io;

use super::{Foreground, TerminalSize};

/// The macOS and Linux implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

impl TerminalSize for Native {
    fn of_standard_output(&self) -> io::Result<(u16, u16)> {
        let mut size = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: `size` is a `winsize` the call writes into, alive for it.
        if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok((size.ws_col, size.ws_row))
    }
}

impl Foreground for Native {
    fn owns_standard_input(&self) -> bool {
        // SAFETY: plain queries of this process's group and its standard
        // input's terminal; tcgetpgrp answers -1 when that is no terminal.
        let foreground = unsafe { libc::tcgetpgrp(libc::STDIN_FILENO) };
        foreground != -1 && foreground == unsafe { libc::getpgrp() }
    }
}
