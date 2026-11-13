//! How a release is put in place: the running `floti` binary, and the
//! desktop app, installed fresh or over the one there.
//!
//! - **The binary** is put in place by
//!   [`platform::standalone`](crate::platform::standalone)
//!   (`Standalone::replace`), as `floti install-self` puts it, and found
//!   there by `Standalone::binary`. A floti inside the desktop app's
//!   install is the app's ([`Installs::bundles`]) and is never replaced on
//!   its own: the app's installer brings its own.
//! - **The desktop app.** Where it is installed, which version it is, and
//!   how it is installed once its process has exited. Windows: the per-user
//!   NSIS install `electron-builder.yml` makes,
//!   `%LOCALAPPDATA%\Programs\Floti\desktop` with `Floti.exe` and
//!   `Uninstall Floti.exe`, its version the `DisplayVersion` of its uninstall
//!   entry, installed or updated by running the release's `setup.exe`
//!   silently (`/S`), out of any job the caller is in. macOS: `Floti.app` in
//!   `~/Applications` or `/Applications`, its version
//!   `CFBundleShortVersionString` in its `Info.plist`, replaced by (or
//!   installed in `~/Applications` as) the one in the release's zip,
//!   extracted beside it with `ditto --noqtn` and moved in by rename. Linux:
//!   the AppImage at `~/Applications/Floti.AppImage`, its version unknown,
//!   written beside, made executable and renamed into place.
//!
//!   An install from before the app was renamed Floti is found too, under
//!   its old name (`Programs\Helm`, `Helm.app`, `Helm.AppImage`), and an
//!   update puts the new app in its new place and removes the old one
//!   ([`Installs::install_desktop`] answers where the app is then).
//!   Opened again by its program on Windows, `open` on macOS and the
//!   AppImage itself on Linux ([`Installs::opener`]), there with the display
//!   and session-bus variables the app ran with ([`Installs::display_env`]).
//!
//! A first install (`floti add desktop`) also needs a launcher entry and a
//! display. The entry ([`Installs::add_launcher`]): the installer makes the
//! Start menu's on Windows, `~/Applications` is the Applications entry on
//! macOS, and on Linux floti writes a freedesktop entry and the app's icon
//! under `$XDG_DATA_HOME` (`~/.local/share`). The display
//! ([`Installs::no_display`]): Linux has none without `DISPLAY` or
//! `WAYLAND_DISPLAY`, Windows none in session 0, where ssh logins and
//! services run, and macOS none when launchd has no GUI domain for the user,
//! nobody being signed in at the screen. Setup's question also leaves out a
//! session the app could run in but does not belong in
//! ([`Installs::not_offered`]): a WSL distro, whose desktop is Windows'.
//!
//! `floti uninstall` removes the app the OS's way ([`Installs::removal`]):
//! on Windows by its own NSIS uninstaller, run silently (`/S`) and in place
//! (`_?=<install folder>`), so the wait is for the uninstall itself rather
//! than for the copy NSIS otherwise starts in `%TEMP%` and does not wait for;
//! the uninstaller left behind then goes with its folder. On macOS the
//! `.app`, and on Linux the AppImage with its menu entry and icon, are
//! removed as files.
//!
//! Nothing here adds a quarantine flag or a download mark: the files are
//! written by floti's own process, which neither Gatekeeper's quarantine nor
//! Windows' mark of the web applies to, and the macOS extraction is told
//! (`--noqtn`) not to carry one over from the archive.
//!
//! In a build with the testkit only, `FLOTI_DESKTOP_DIR` ([`DESKTOP_DIR_VAR`])
//! moves the folder the app is looked for and installed in, on every OS, so
//! no test reaches the real ones, and `FLOTI_TEST_DISPLAY`
//! ([`DISPLAY_TEST_VAR`]) says whether there is a display. A release build
//! has no such switch.
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

/// Says whether there is a display, in a build with the testkit only:
/// `none` or `present`.
#[cfg(any(test, feature = "testkit"))]
pub const DISPLAY_TEST_VAR: &str = "FLOTI_TEST_DISPLAY";

/// The desktop app: where it is installed, or where a new install goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desktop {
    /// What a person calls it: `Floti`, or `Helm` for an install from before
    /// the rename, which its program, bundle or AppImage is named after.
    pub name: &'static str,
    /// The install: its folder on Windows, the `.app` on macOS, the
    /// AppImage on Linux.
    pub path: PathBuf,
}

/// What removing the installed desktop app takes, for `floti uninstall`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopRemoval {
    /// What the line that says it was removed names: the install folder,
    /// the `.app` or the AppImage.
    pub shown: PathBuf,
    /// The app's own uninstaller and its arguments as one line, appended to
    /// its command line as written, run silently and waited for before
    /// anything else: on Windows the NSIS uninstaller, run in place
    /// (`/S _?=<folder>`) so the wait is for the uninstall itself. NSIS reads
    /// `_?=` to the end of the line and takes no quotes, so the line is never
    /// quoted.
    pub uninstaller: Option<(PathBuf, String)>,
    /// A file the uninstaller must have removed, or it failed: the app's
    /// program on Windows.
    pub gone_after: Option<PathBuf>,
    /// What is removed then, each without walking into a link: what the
    /// uninstaller leaves of its folder, the app itself and its menu entry.
    pub remove: Vec<PathBuf>,
}

/// The OS's way to put a release in place.
pub trait Installs {
    /// Whether `running`, a floti binary, is part of `desktop`'s install,
    /// which brings its own floti with every update.
    fn bundles(&self, desktop: &Desktop, running: &Path) -> bool;

    /// The desktop app installed for the user `env` describes, or `None`.
    fn desktop(&self, env: &UserEnv) -> Option<Desktop>;

    /// The installed desktop app, or, with none, where a new install goes;
    /// `None` when `env` names no folder for it.
    fn desktop_target(&self, env: &UserEnv) -> Option<Desktop>;

    /// The installed `desktop`'s version, or `None` when it cannot be told.
    fn desktop_version(&self, desktop: &Desktop) -> Option<String>;

    /// The floti `desktop`'s install carries, for `floti update` to run
    /// `install-self` from once it has installed the app, so the Node beside
    /// floti installed on its own is the new app's: `None` where the app's
    /// own installer runs it (Windows) or the floti is inside an image that
    /// is not mounted (the AppImage, whose app runs it when it starts).
    fn carried_floti(&self, desktop: &Desktop) -> Option<PathBuf>;

    /// Whether `row` is one of `desktop`'s processes. A row the OS gave no
    /// command line for is judged by its name alone, so an app whose
    /// command line could not be read counts as running.
    fn is_desktop_process(&self, desktop: &Desktop, row: &ProcessRow) -> bool;

    /// Installs the release's file `release` at `target`, over the app
    /// there or fresh, once the app has quit, and answers the app as it is
    /// installed then: `target`, or, where `target` was installed under the
    /// name it had before Floti, the app in its new place, the old one
    /// removed. When it fails before the swap, an installed app is as it was.
    fn install_desktop(&self, target: &Desktop, release: &Path) -> Result<Desktop>;

    /// Carries the launcher entry floti wrote for `replaced`, an install from
    /// before the rename, over to `installed`, the app an update put in its
    /// place, and answers the entry it wrote: on Linux the old menu entry and
    /// icon are removed and the new ones written, where floti had written
    /// them. Nothing elsewhere, where the OS's install makes the entry.
    fn carry_launcher(
        &self,
        env: &UserEnv,
        replaced: &Desktop,
        installed: &Desktop,
    ) -> Result<Option<PathBuf>> {
        let _ = (env, replaced, installed);
        Ok(None)
    }

    /// The program, and its arguments, that open the installed `desktop`:
    /// what `floti update --from-desktop` starts once it has replaced the app
    /// that quit for it.
    fn opener(&self, desktop: &Desktop) -> (PathBuf, Vec<String>);

    /// The display and session-bus variables the app's running process
    /// `pid` has, for opening it again where the process that does so has
    /// none: a host started by the systemd user unit is not in the
    /// graphical session. Only [`DISPLAY_VARS`] are taken, from the process
    /// `floti update` found to be the installed app. Nothing where the OS
    /// opens an app in the user's session without them (Windows, and
    /// `open` on macOS).
    fn display_env(&self, pid: u32) -> Vec<(String, String)> {
        let _ = pid;
        Vec::new()
    }

    /// Why the session `env` describes has no display to open the app on,
    /// as a clause ("this session has no ..."), or `None` when it has one
    /// or that cannot be told cheaply.
    fn no_display(&self, env: &UserEnv) -> Option<String> {
        let _ = env;
        None
    }

    /// Why `floti setup` does not ask about the app in the session `env`
    /// describes, though it has a display and `floti add desktop` would
    /// install it there, as a clause, or `None` where it asks.
    fn not_offered(&self, env: &UserEnv) -> Option<String> {
        let _ = env;
        None
    }

    /// Adds the launcher entry for `desktop`, just installed for the user
    /// `env` describes, where the OS's installer does not make one, and
    /// answers the entry it wrote: nothing on Windows (the installer makes
    /// the Start menu's) and macOS (`~/Applications` is the Applications
    /// entry).
    fn add_launcher(&self, env: &UserEnv, desktop: &Desktop) -> Result<Option<PathBuf>> {
        let _ = (env, desktop);
        Ok(None)
    }

    /// Adds the launcher entry for the installed `desktop` where floti makes
    /// one ([`Installs::add_launcher`]) and it is missing, and answers the
    /// entry it wrote; nothing where it is there or the OS's install makes
    /// it.
    fn repair_launcher(&self, env: &UserEnv, desktop: &Desktop) -> Result<Option<PathBuf>> {
        let _ = (env, desktop);
        Ok(None)
    }

    /// What removing the desktop app installed for the user `env` describes
    /// takes, or `None` when there is nothing of it to remove: on Windows
    /// its NSIS uninstaller, run silently in place, then what that leaves of
    /// the install folder; on macOS the `.app`; on Linux the AppImage, and
    /// the menu entry and icon [`Installs::add_launcher`] wrote, whichever
    /// of them are there.
    fn removal(&self, env: &UserEnv) -> Option<DesktopRemoval> {
        let _ = env;
        None
    }
}

/// Why there is no display for the app in the session `env` describes
/// ([`Installs::no_display`]), or, in a build with the testkit, what
/// [`DISPLAY_TEST_VAR`] says.
pub fn missing_display(installs: &dyn Installs, env: &UserEnv) -> Option<String> {
    #[cfg(any(test, feature = "testkit"))]
    if let Some(said) = env.var(DISPLAY_TEST_VAR) {
        return (said == "none").then(|| "this test's session has no display".to_owned());
    }
    installs.no_display(env)
}

/// Why `floti setup` does not ask about the app in the session `env`
/// describes ([`Installs::not_offered`]), or, in a build with the testkit
/// where [`DISPLAY_TEST_VAR`] says what the session is, nothing: the test's
/// session is what that says, whatever computer runs the test.
pub fn not_offered(installs: &dyn Installs, env: &UserEnv) -> Option<String> {
    #[cfg(any(test, feature = "testkit"))]
    if env.var(DISPLAY_TEST_VAR).is_some() {
        return None;
    }
    installs.not_offered(env)
}

/// The variables [`Installs::display_env`] takes: the display, its
/// authority, and the session bus.
pub const DISPLAY_VARS: [&str; 4] = [
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// The [`DISPLAY_VARS`] in `environ`, a process's environment as
/// `/proc/<pid>/environ` holds it: `NAME=value` entries ending in NUL. An
/// entry that is not UTF-8 or has no value is left out.
pub fn display_vars(environ: &[u8]) -> Vec<(String, String)> {
    environ
        .split(|byte| *byte == 0)
        .filter_map(|entry| std::str::from_utf8(entry).ok())
        .filter_map(|entry| entry.split_once('='))
        .filter(|(name, value)| DISPLAY_VARS.contains(name) && !value.is_empty())
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
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
    /// its file name and its bytes. `installer` is a program that records
    /// how it was run: on Windows the release file is it, and on macOS the
    /// app carries it as its floti.
    fn fake_release(&self, work: &Path, version: &str, installer: &Path) -> (String, Vec<u8>);

    /// Whether the fake install in `apps` was updated to `version`. On
    /// Windows that is the installer having run silently, as `record`, the
    /// installer's record of its runs, says.
    fn was_updated(&self, apps: &Path, version: &str, record: &Path) -> bool;
}
