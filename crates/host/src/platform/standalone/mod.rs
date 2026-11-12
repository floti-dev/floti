//! floti installed on its own: the folder it lives in, where the install
//! scripts put it, and putting a new copy of a file there in place of one
//! that may be running.
//!
//! - Windows: `%LOCALAPPDATA%\Programs\Floti\bin`, and no folder where
//!   `%LOCALAPPDATA%` is not set. A running `.exe` can be
//!   renamed but neither replaced nor removed, so the copy there is renamed
//!   aside (`floti.old-<pid>-<n>.exe`) and the new one moved into its name,
//!   as `scripts/install/install.ps1` does; the process running the old copy
//!   keeps running from the copy aside, which is removed at once when nothing
//!   runs it. Copies left aside are removed by a later install or update,
//!   once nothing runs them: Windows refuses to remove one that a process
//!   still runs, and that refusal is the test. Each rename waits out, for a
//!   few seconds, a file something holds for a moment, such as Defender
//!   scanning the just-written copy.
//! - macOS and Linux: `~/.local/bin`. The new file is renamed over the old
//!   one, which leaves a process running it its own copy, so nothing is ever
//!   put aside. On macOS the new file loses `com.apple.quarantine` first: a
//!   copy out of an app opened from a downloaded dmg carries it.
//!
//! Each OS's rule is plain path arithmetic and renames, so all three are
//! compiled and tested on every OS; the build picks one as [`Native`].

pub mod linux;
pub mod macos;
mod posix;
pub mod windows;

#[cfg(target_os = "linux")]
pub use linux::Native;
#[cfg(target_os = "macos")]
pub use macos::Native;
#[cfg(target_os = "windows")]
pub use windows::Native;

use std::io;
use std::path::{Path, PathBuf};

use super::folders::UserEnv;

#[cfg(test)]
mod tests;

/// One OS's place for floti installed on its own, and its way to replace a
/// file there.
pub trait Standalone {
    /// The folder floti installed on its own lives in for the user `env`
    /// describes, whether it is there or not; `None` when `env` names no
    /// folder to put it in.
    fn folder(&self, env: &UserEnv) -> Option<PathBuf>;

    /// floti installed on its own in [`Standalone::folder`], there or not:
    /// `floti.exe` on Windows, `floti` elsewhere.
    fn binary(&self, env: &UserEnv) -> Option<PathBuf>;

    /// Puts `new`, a complete file in `target`'s folder, in `target`'s
    /// place, `target` there or not, executable and, on macOS, with no
    /// quarantine flag. When it fails, `target` is as it was. Both
    /// `floti install-self` and `floti update` put floti in place through it.
    fn replace(&self, target: &Path, new: &Path) -> io::Result<()>;

    /// Removes the copies of `target` an earlier [`Standalone::replace`] put
    /// aside, where nothing runs them any more, and answers those it left.
    fn remove_asides(&self, target: &Path) -> Vec<PathBuf>;
}
