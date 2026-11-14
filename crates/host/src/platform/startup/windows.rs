//! How the host starts on Windows: when a client needs one, never at sign-in.
//! The desktop app starts it when no host answers, and `floti connect` when a
//! connection finds none; each starts it detached, with no console
//! (`ProcessTree::start_detached`), so no window opens and none can be
//! closed to stop it. Nothing is registered with Task Scheduler.
//!
//! `floti connect` runs over the user's own `ssh <target> floti connect`, so
//! a start from an SSH login is no refusal here, as it is where the host
//! starts at sign-in. The host it starts gets no `SSH_*` variables, so
//! neither it nor the sessions it starts look like an SSH login. OpenSSH runs
//! the login in session 0, with no desktop, and an administrator's with the
//! full, elevated token. An elevated login starts no host while the user has
//! a desktop session ([`ELEVATED_AT_DESKTOP`]): Floti opened at that desktop
//! starts one as the user is signed in there. With nobody signed in it
//! starts the host, whose pipe and files admit the same user's non-elevated
//! client by their label (`socket/windows.rs`).
//!
//! An earlier floti did register the logon task "Floti host", which ran the
//! console program `floti.exe host` and so opened a console window at every
//! sign-in. [`LEFTOVER_TASK`] names it, so `floti setup`, `floti update` and
//! `floti uninstall` delete it ([`register::remove_leftover_task`]).
//!
//! [`register::remove_leftover_task`]: super::register::remove_leftover_task

use std::path::Path;

use super::{Account, HostStart, HostStartup, LeftoverTask, Login, OnConnect, SSH_VARS};
use crate::Result;
use crate::platform::folders::UserEnv;

/// The logon task an earlier floti registered, and the XML it imported, kept
/// under the data folder.
pub const LEFTOVER_TASK: LeftoverTask = LeftoverTask {
    name: "Floti host",
    file_name: "Floti host.xml",
};

/// Why `floti connect` in an elevated login starts no host while the user
/// has a desktop session on the PC.
pub const ELEVATED_AT_DESKTOP: &str = "no Floti host is running there, and an administrator's SSH login does not start one while you are signed in at that PC, so open Floti on that PC.";

/// The Windows implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

impl HostStartup for Native {
    fn host_start(&self, _binary: &Path) -> Result<HostStart> {
        Ok(HostStart::WhenNeeded {
            leftover: LEFTOVER_TASK,
        })
    }

    /// Only a service account is refused: a start from SSH is how a client
    /// on another computer starts the host.
    fn launch_refusal(&self, _env: &UserEnv, account: Account) -> Option<String> {
        match account {
            Account::Privileged(name) => Some(format!(
                "The Floti host runs as you, not as {name}, so it was not started."
            )),
            Account::User | Account::Unknown => None,
        }
    }

    /// Always starts one, without the SSH variables, except from an elevated
    /// login while the user has a desktop session. Elevation or a desktop
    /// session Windows would not tell of counts as none, so the host starts.
    fn on_connect(&self, _data: &Path, login: &dyn Login) -> OnConnect {
        if login.elevated() == Some(true) && login.desktop_session().unwrap_or(false) {
            OnConnect::Refuse(ELEVATED_AT_DESKTOP)
        } else {
            OnConnect::Start {
                env_remove: &SSH_VARS,
            }
        }
    }
}
