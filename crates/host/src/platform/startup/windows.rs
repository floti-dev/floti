//! How the host starts on Windows: when a client needs one, never at sign-in.
//! The desktop app starts it when no host answers, and `floti connect` when a
//! connection finds none; each starts it detached, with no console
//! (`ProcessTree::start_detached`), so no window opens and none can be
//! closed to stop it. Nothing is registered with Task Scheduler.
//!
//! An earlier floti did register the logon task "Floti host", which ran the
//! console program `floti.exe host` and so opened a console window at every
//! sign-in. [`LEFTOVER_TASK`] names it, so `floti setup`, `floti update` and
//! `floti uninstall` delete it ([`register::remove_leftover_task`]).
//!
//! [`register::remove_leftover_task`]: super::register::remove_leftover_task

use std::path::Path;

use super::{HostStart, HostStartup, LeftoverTask};
use crate::Result;

/// The logon task an earlier floti registered, and the XML it imported, kept
/// under the data folder.
pub const LEFTOVER_TASK: LeftoverTask = LeftoverTask {
    name: "Floti host",
    file_name: "Floti host.xml",
};

/// The Windows implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

impl HostStartup for Native {
    fn host_start(&self, _binary: &Path) -> Result<HostStart> {
        Ok(HostStart::WhenNeeded {
            leftover: LEFTOVER_TASK,
        })
    }
}
