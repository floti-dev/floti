//! The host's place in the user's sign-in on macOS and Linux: whether it
//! runs as root, and the signals that stop it (SIGINT for Ctrl+C, SIGTERM
//! from launchd or systemd when the user signs out, SIGHUP when its terminal
//! goes).

use tokio::signal::unix::{Signal, SignalKind, signal};

use super::Account;
use crate::{Error, Result};

pub(super) fn account() -> Account {
    // SAFETY: geteuid has no preconditions and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        Account::Privileged("root")
    } else {
        Account::User
    }
}

/// Windows' build number, which no other OS has.
pub(super) fn windows_build() -> Option<u32> {
    None
}

/// Whether this user has a desktop session here. Only Windows' rules ask: on
/// macOS setup asks launchd for the user's GUI domain, and on Linux the
/// user's own service manager starts the unit.
pub(super) fn desktop_session() -> std::io::Result<bool> {
    Ok(true)
}

/// Whether this process's token is elevated, which only Windows has: root is
/// an account here ([`account`]), not an elevation.
pub(super) fn elevated() -> Option<bool> {
    None
}

/// The signals that stop the host, registered while this lives.
pub(super) struct Signals {
    interrupt: Signal,
    terminate: Signal,
    hangup: Signal,
}

impl Signals {
    pub(super) fn register() -> Result<Self> {
        let listen = |kind| {
            signal(kind).map_err(|err| Error::io("listen for the signals that stop it", err))
        };
        Ok(Self {
            interrupt: listen(SignalKind::interrupt())?,
            terminate: listen(SignalKind::terminate())?,
            hangup: listen(SignalKind::hangup())?,
        })
    }

    pub(super) async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.interrupt.recv() => "Ctrl+C",
            _ = self.terminate.recv() => "SIGTERM",
            _ = self.hangup.recv() => "SIGHUP",
        }
    }

    pub(super) fn stopped(&self) -> Stopped {
        Stopped
    }
}

/// Nothing waits on the host's stop here: SIGTERM leaves it the time it
/// takes, until launchd or systemd gives up on it.
pub(super) struct Stopped;

/// Makes `command` a process [`interrupt`] can reach. Any process can be
/// signalled, so there is nothing to do.
#[cfg(any(test, feature = "testkit"))]
pub fn stoppable(_command: &mut std::process::Command) {}

/// Sends process `pid` SIGINT, as Ctrl+C in its terminal would.
#[cfg(any(test, feature = "testkit"))]
pub fn interrupt(pid: u32) -> std::io::Result<()> {
    send(pid, libc::SIGINT)
}

/// Sends process `pid` SIGTERM, as launchd and systemd do when the user
/// signs out.
#[cfg(any(test, feature = "testkit"))]
pub fn terminate(pid: u32) -> std::io::Result<()> {
    send(pid, libc::SIGTERM)
}

#[cfg(any(test, feature = "testkit"))]
fn send(pid: u32, signal: libc::c_int) -> std::io::Result<()> {
    let pid = libc::pid_t::try_from(pid)
        .ok()
        .filter(|pid| *pid > 1)
        .ok_or_else(|| std::io::Error::other("no process to signal"))?;
    // SAFETY: plain call on a positive pid.
    if unsafe { libc::kill(pid, signal) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Makes `command` start with no console. macOS and Linux give no process
/// one, so there is nothing to do.
#[cfg(any(test, feature = "testkit"))]
pub fn without_console(_command: &mut std::process::Command) {}

/// The windows a process made for the OS to tell it the session is ending:
/// macOS and Linux tell a process with a signal, so it has none to find.
#[cfg(any(test, feature = "testkit"))]
pub fn top_level_windows(_pid: u32, _class: &str) -> floti_protocol::Knowable<Vec<isize>> {
    floti_protocol::Knowable::Unknown
}

/// macOS and Linux send no window messages when the session ends.
#[cfg(any(test, feature = "testkit"))]
pub fn send_session_message(
    _window: isize,
    _message: super::SessionMessage,
    _within: std::time::Duration,
) -> std::io::Result<isize> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "this OS sends no window messages when the session ends",
    ))
}
