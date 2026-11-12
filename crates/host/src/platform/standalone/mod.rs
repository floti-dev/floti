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
//! Uninstalling (`floti uninstall`) takes floti out of that place the same
//! way. On Windows the running `floti.exe` is renamed aside, since it can be
//! neither removed nor replaced while it runs, and a hidden Windows
//! PowerShell started apart from it deletes the copy aside once nothing runs
//! it, then the folders the install made for floti alone
//! ([`Standalone::own_folders`]: `bin` and `Floti` above it) once they are
//! empty. It needs no admin rights (a delete at the next restart does), leaves
//! no copy of floti behind to delete itself, and waits on Windows' own refusal
//! to remove a file a process runs, the same test [`Standalone::remove_asides`]
//! uses. What it could not delete in its few minutes, a later install or
//! update sweeps as a copy put aside. On macOS and Linux the file is unlinked,
//! which a process running it survives, and `~/.local/bin` is the user's, so
//! nothing is left for later.
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
use super::process_tree::Outliving;

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

    /// This computer's own CPU, as the release's file names say it (`x64`,
    /// `arm64`), whatever this floti was built for: on Windows the native
    /// machine `IsWow64Process2` names, so an x64 floti emulated on an Arm
    /// computer says `arm64`; on macOS and Linux this build's own.
    fn native_cpu(&self) -> &'static str;

    /// The folders an install made for floti alone, deepest first, which
    /// uninstalling removes once they are empty: on Windows
    /// `%LOCALAPPDATA%\Programs\Floti\bin` and `Floti` above it; none on
    /// macOS and Linux, where `~/.local/bin` is the user's own.
    fn own_folders(&self, env: &UserEnv) -> Vec<PathBuf>;

    /// Removes `target`, which may be the file this process runs from, and
    /// answers the copy of it still to delete once nothing runs it, if any.
    /// On Windows a running file is renamed aside, as
    /// [`Standalone::replace`] does, so its name is free at once; on macOS
    /// and Linux it is unlinked, which a process running it survives. A
    /// `target` that is not there is already removed.
    fn remove_running(&self, target: &Path) -> io::Result<Option<PathBuf>>;

    /// Deletes each of `files`, then each of `folders` that is empty and not
    /// a link, once this process has exited, for the user `env` describes.
    /// On Windows a hidden Windows PowerShell, started apart from this
    /// process and holding none of its handles, does it, out of this
    /// process's job or, where the job refuses, inside it; the answer says
    /// which ([`Outliving`]). On macOS and Linux nothing a process runs needs
    /// waiting for, and it is done at once ([`Outliving::Apart`]).
    fn delete_after_exit(
        &self,
        env: &UserEnv,
        files: &[PathBuf],
        folders: &[PathBuf],
    ) -> io::Result<Outliving>;
}
