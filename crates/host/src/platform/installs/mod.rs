//! How a release is put in place: the running `floti` binary, and the
//! desktop app, installed fresh or over the one there.
//!
//! - **The binary.** On Windows a running `.exe` can be renamed but not
//!   replaced, so the running one is renamed aside (`floti.old.exe`) and the
//!   new one moved into its name, as `scripts/install/install.ps1` does; the
//!   old copy is removed by the next update, once nothing runs it. On macOS
//!   and Linux the new file, already written beside the running one, is
//!   made executable and renamed over it, which leaves the running process
//!   its own copy. A floti inside the desktop app's install is the app's
//!   ([`Installs::bundles`]) and is never replaced on its own: the app's
//!   installer brings its own.
//! - **The desktop app.** Where it is installed, which version it is, and
//!   how it is installed once its process has exited. Windows: the per-user
//!   NSIS install `electron-builder.yml` makes, `%LOCALAPPDATA%\Programs\Helm`
//!   with `Helm.exe` and `Uninstall Helm.exe`, its version the
//!   `DisplayVersion` of its uninstall entry, installed or updated by
//!   running the release's `setup.exe` silently (`/S`), out of any job the
//!   caller is in. macOS: `Helm.app` in `~/Applications` or `/Applications`,
//!   its version `CFBundleShortVersionString` in its `Info.plist`, replaced
//!   by (or installed in `~/Applications` as) the one in the release's zip,
//!   extracted beside it with `ditto --noqtn` and moved in by rename. Linux:
//!   the AppImage at `~/Applications/Helm.AppImage`, its version unknown,
//!   written beside, made executable and renamed into place.
//!
//! Nothing here adds a quarantine flag or a download mark: the files are
//! written by floti's own process, which neither Gatekeeper's quarantine nor
//! Windows' mark of the web applies to, and the macOS extraction is told
//! (`--noqtn`) not to carry one over from the archive.
//!
//! In a build with the testkit only, `FLOTI_DESKTOP_DIR` ([`DESKTOP_DIR_VAR`])
//! moves the folder the app is looked for and installed in, on every OS, so
//! no test reaches the real ones. A release build has no such switch.
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

/// Moves the folder the desktop app is looked for and installed in, in a
/// build with the testkit only.
#[cfg(any(test, feature = "testkit"))]
pub const DESKTOP_DIR_VAR: &str = "FLOTI_DESKTOP_DIR";

/// The desktop app: where it is installed, or where a new install goes.
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

    /// Whether `running`, a floti binary, is part of `desktop`'s install,
    /// which brings its own floti with every update.
    fn bundles(&self, desktop: &Desktop, running: &Path) -> bool;

    /// Where floti installed on its own is for the user `env` describes,
    /// whether it is there or not: where the install scripts put it.
    fn standalone_binary(&self, env: &UserEnv) -> Option<PathBuf>;

    /// The desktop app installed for the user `env` describes, or `None`.
    fn desktop(&self, env: &UserEnv) -> Option<Desktop>;

    /// The installed desktop app, or, with none, where a new install goes;
    /// `None` when `env` names no folder for it.
    fn desktop_target(&self, env: &UserEnv) -> Option<Desktop>;

    /// The installed `desktop`'s version, or `None` when it cannot be told.
    fn desktop_version(&self, desktop: &Desktop) -> Option<String>;

    /// Whether installing the desktop app over an installed one stops the
    /// host on this computer, ending the sessions it runs.
    fn installing_desktop_stops_host(&self) -> bool;

    /// Whether `row` is one of `desktop`'s processes. A row the OS gave no
    /// command line for is judged by its name alone, so an app whose
    /// command line could not be read counts as running.
    fn is_desktop_process(&self, desktop: &Desktop, row: &ProcessRow) -> bool;

    /// Installs the release's file `release` at `target`, over the app
    /// there or fresh, once the app has quit. When it fails before the
    /// swap, an installed app is as it was.
    fn install_desktop(&self, target: &Desktop, release: &Path) -> Result<()>;
}

/// The folder `env`'s [`DESKTOP_DIR_VAR`] names, in a build with the
/// testkit; never in a release build.
fn desktop_dir_override(env: &UserEnv) -> Option<PathBuf> {
    #[cfg(any(test, feature = "testkit"))]
    return env.absolute_var(DESKTOP_DIR_VAR);
    #[cfg(not(any(test, feature = "testkit")))]
    {
        let _ = env;
        None
    }
}

/// The text of the first `<string>` after `<key>name</key>` in the XML
/// property list `plist`.
fn plist_string(plist: &str, name: &str) -> Option<String> {
    let key = format!("<key>{name}</key>");
    let after = &plist[plist.find(&key)? + key.len()..];
    let after = after.trim_start();
    let value = after.strip_prefix("<string>")?;
    let value = &value[..value.find("</string>")?];
    Some(value.trim().to_owned()).filter(|value| !value.is_empty())
}

/// `path`'s file name, as text.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What a new copy of the app is called beside it while it is unpacked or
/// written, before it is moved into place.
const NEW_COPY: &str = "floti-update";

/// What the app being replaced is called beside it, until it is removed.
const OLD_COPY: &str = "floti-old";

/// A name beside `path` that this process alone uses: `.<name>.<what>-<pid>`.
fn beside(path: &Path, what: &str) -> PathBuf {
    path.with_file_name(format!(
        ".{}.{what}-{}",
        file_name(path),
        std::process::id()
    ))
}

/// What an install of `desktop` left beside it, each with the id of the
/// process that made it: a new copy or an old one ([`beside`]) that an
/// install ended before removing.
pub fn leftovers(desktop: &Desktop) -> Vec<(PathBuf, u32)> {
    let Some(folder) = desktop.path.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let prefix = format!(".{}.", file_name(&desktop.path));
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let rest = name.to_str()?.strip_prefix(&prefix)?;
            let pid = [NEW_COPY, OLD_COPY].iter().find_map(|what| {
                rest.strip_prefix(what)?
                    .strip_prefix('-')?
                    .parse::<u32>()
                    .ok()
            })?;
            Some((entry.path(), pid))
        })
        .collect()
}

/// What a test needs to stand a fake desktop install and a fake release of
/// it up, on whichever OS it runs, without naming the OS.
#[cfg(any(test, feature = "testkit"))]
pub trait FakeDesktop {
    /// Makes a fake install of the desktop app at `version` in `apps`, the
    /// folder [`DESKTOP_DIR_VAR`] names. Where the OS keeps the version
    /// outside the install (Windows), the fake's version is unknown.
    fn fake_install(&self, apps: &Path, version: &str);

    /// A fake release file of the desktop app at `version`, made in `work`:
    /// its file name and its bytes. On Windows it is `installer`, a program
    /// that records how it was run.
    fn fake_release(&self, work: &Path, version: &str, installer: &Path) -> (String, Vec<u8>);

    /// Whether the fake install in `apps` was updated to `version`. On
    /// Windows that is the installer having run silently, as `record`, the
    /// installer's record of its runs, says.
    fn was_updated(&self, apps: &Path, version: &str, record: &Path) -> bool;
}
