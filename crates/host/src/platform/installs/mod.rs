//! How `floti update` puts a release in place: the running `floti` binary,
//! and the desktop app where it is installed.
//!
//! - **The binary.** On Windows a running `.exe` can be renamed but not
//!   replaced, so the running one is renamed aside (`floti.old.exe`) and the
//!   new one moved into its name, as `scripts/install/install.ps1` does; the
//!   old copy is removed by the next update, once nothing runs it. On macOS
//!   and Linux the new file, already written beside the running one, is
//!   made executable and renamed over it, which leaves the running process
//!   its own copy.
//! - **The desktop app.** Where it is installed and how it is replaced once
//!   its process has exited. Windows: the per-user NSIS install
//!   `electron-builder.yml` makes, `%LOCALAPPDATA%\Programs\Helm` with
//!   `Helm.exe` and `Uninstall Helm.exe`, updated by running the release's
//!   `setup.exe` silently (`/S`). macOS: `Helm.app` in `~/Applications` or
//!   `/Applications`, replaced by the one in the release's zip, extracted
//!   beside it with `ditto --noqtn` and swapped in by rename. Linux: the
//!   AppImage at `~/Applications/Floti.AppImage`, where `floti add desktop`
//!   installs it, replaced by writing the new one beside it, making it
//!   executable and renaming it over.
//!
//! `FLOTI_DESKTOP_DIR` ([`DESKTOP_DIR_VAR`]) moves the folder the app is
//! looked for in, on every OS: `%LOCALAPPDATA%\Programs` on Windows, the two
//! Applications folders on macOS, `~/Applications` on Linux. Tests use it so
//! nothing reaches the real ones.
//!
//! Nothing here adds a quarantine flag or a download mark: the files are
//! written by floti's own process, which neither Gatekeeper's quarantine nor
//! Windows' mark of the web applies to, and the macOS extraction is told
//! (`--noqtn`) not to carry one over from the archive.
//!
//! Each OS's file compiles on every OS, so its logic is tested everywhere;
//! the build picks one as [`Native`].

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

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use crate::Result;
use crate::platform::folders::UserEnv;
use crate::platform::process_table::ProcessRow;

/// Moves the folder the desktop app is looked for in.
pub const DESKTOP_DIR_VAR: &str = "FLOTI_DESKTOP_DIR";

/// The desktop app, where it is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desktop {
    /// What a person calls it: `Helm` until the switch to Floti renames it.
    pub name: &'static str,
    /// The install: its folder on Windows, the `.app` on macOS, the
    /// AppImage on Linux.
    pub path: PathBuf,
}

/// The OS's way to put a release in place.
pub trait Installs {
    /// Puts `new`, a checked file written beside `running`, in `running`'s
    /// place. When it fails, `running` is as it was.
    fn replace_binary(&self, running: &Path, new: &Path) -> Result<()>;

    /// The desktop app installed for the user `env` describes, or `None`.
    fn desktop(&self, env: &UserEnv) -> Option<Desktop>;

    /// Whether `row` is one of `desktop`'s processes. A row the OS gave no
    /// command line for is judged by its name alone, so an app whose
    /// command line could not be read counts as running.
    fn is_desktop_process(&self, desktop: &Desktop, row: &ProcessRow) -> bool;

    /// Replaces `desktop` with the release's file `release`, once the app
    /// has quit. When it fails before the swap, the app is as it was.
    fn replace_desktop(&self, desktop: &Desktop, release: &Path) -> Result<()>;
}

/// The folder `env`'s [`DESKTOP_DIR_VAR`] names, when it names one.
fn desktop_dir_override(env: &UserEnv) -> Option<PathBuf> {
    env.absolute_var(DESKTOP_DIR_VAR)
}

/// `path`'s file name, as text.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A name beside `path` that this process alone uses: `.<name>.<what>-<pid>`.
fn beside(path: &Path, what: &str) -> PathBuf {
    path.with_file_name(format!(
        ".{}.{what}-{}",
        file_name(path),
        std::process::id()
    ))
}

/// What a test needs to stand a fake desktop install and a fake release of
/// it up, on whichever OS it runs, without naming the OS.
#[cfg(any(test, feature = "testkit"))]
pub trait FakeDesktop {
    /// Makes a fake install of the desktop app in `apps`, the folder
    /// [`DESKTOP_DIR_VAR`] names.
    fn fake_install(&self, apps: &Path);

    /// A fake release file of the desktop app at `version`, made in `work`:
    /// its file name and its bytes. On Windows it is `installer`, a program
    /// that records how it was run.
    fn fake_release(&self, work: &Path, version: &str, installer: &Path) -> (String, Vec<u8>);

    /// Whether the fake install in `apps` was updated to `version`. On
    /// Windows that is the installer having run silently, as `record`, the
    /// installer's record of its runs, says.
    fn was_updated(&self, apps: &Path, version: &str, record: &Path) -> bool;
}
