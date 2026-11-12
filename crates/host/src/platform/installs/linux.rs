//! Linux: the binary is renamed over; the desktop app is the AppImage at
//! `~/Applications/Helm.AppImage`, written beside and renamed into place in
//! the same way.
//!
//! `~/Applications` is where AppImage's own integration tools look for
//! AppImages. It is outside Floti's data folder, which `floti uninstall`
//! keeps. An AppImage carries no version floti can read without running
//! it, so its version is unknown and an update always installs it.
//!
//! A first install adds the app to the desktop's menus the freedesktop way:
//! `applications/helm.desktop` and the 256px icon at
//! `icons/hicolor/256x256/apps/helm.png`, both under `$XDG_DATA_HOME`
//! (`~/.local/share` when it is unset or not absolute). The icon is the one
//! the AppImage's own entry has (`build/icon.png`), built into floti, so
//! nothing is read out of the image. Each is written beside and renamed into
//! place. A session has a display when `DISPLAY` or `WAYLAND_DISPLAY` is set.

use std::path::{Path, PathBuf};

use super::{Desktop, Installs, NEW_COPY, beside, desktop_dir_override, file_name, posix};
use crate::platform::folders::UserEnv;
use crate::platform::process_table::ProcessRow;
use crate::{Error, Result};

/// What the desktop app is called: `productName` in `electron-builder.yml`.
pub const DESKTOP_NAME: &str = "Helm";

/// What the menu entry and its icon are called: `executableName` in
/// `electron-builder.yml`.
const ENTRY_NAME: &str = "helm";

/// The menu entry's comment: `linux.description` in `electron-builder.yml`.
const ENTRY_COMMENT: &str =
    "A desktop for Claude Code sessions, with history, files and a browser pane.";

/// The icon the AppImage's own menu entry has.
const ICON: &[u8] = include_bytes!("../../../../../packages/desktop/build/icon.png");

/// The icon's size, as the hicolor theme names its folder.
const ICON_SIZE: &str = "256x256";

/// What an AppImage's runtime names the folder it mounts the image on,
/// followed by part of the image's name: `$TMPDIR/.mount_HelmXXXXXX`.
const MOUNT_PREFIX: &str = ".mount_";

pub struct Native;

impl Native {
    fn app_image_name() -> String {
        format!("{DESKTOP_NAME}.AppImage")
    }

    /// The installed AppImage's path for `env`, whether it is there or not.
    pub fn app_image(env: &UserEnv) -> Option<PathBuf> {
        desktop_dir_override(env)
            .or_else(|| env.home.as_ref().map(|home| home.join("Applications")))
            .map(|folder| folder.join(Self::app_image_name()))
    }

    /// The folder menu entries and icons go under for `env`:
    /// `$XDG_DATA_HOME`, or `~/.local/share`.
    pub fn data_home(env: &UserEnv) -> Option<PathBuf> {
        env.absolute_var("XDG_DATA_HOME").or_else(|| {
            env.home
                .as_ref()
                .map(|home| home.join(".local").join("share"))
        })
    }

    /// The menu entry for the AppImage at `app_image`.
    pub fn entry_text(app_image: &Path) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName={DESKTOP_NAME}\nComment={ENTRY_COMMENT}\nExec={}\nIcon={ENTRY_NAME}\nTerminal=false\nCategories=Development;\nStartupWMClass={DESKTOP_NAME}\n",
            exec_argument(app_image)
        )
    }
}

/// `path` as the program of an entry's `Exec` line: quoted, with `"`, `` ` ``,
/// `$` and `\` escaped as the Exec rules ask, then each `\` doubled as a
/// string value's rules ask, and `%` doubled so it is no field code.
fn exec_argument(path: &Path) -> String {
    let mut quoted = String::from("\"");
    for c in path.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(c);
            }
            '\\' => quoted.push_str("\\\\\\\\"),
            '%' => quoted.push_str("%%"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// Writes `bytes` to `path`, beside it first and renamed into place, making
/// its folder.
fn write_into_place(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|source| Error::path("make", folder, source))?;
    }
    let new = beside(path, NEW_COPY);
    std::fs::write(&new, bytes).map_err(|source| Error::path("write", &new, source))?;
    std::fs::rename(&new, path).map_err(|source| {
        let _ = std::fs::remove_file(&new);
        Error::path("write", path, source)
    })
}

impl Installs for Native {
    fn bundles(&self, _desktop: &Desktop, running: &Path) -> bool {
        // A running AppImage's files are on its read-only mount, not at its
        // path, and the floti it carries runs from there.
        let short: String = DESKTOP_NAME.chars().take(6).collect();
        running.components().any(|part| {
            part.as_os_str()
                .to_str()
                .and_then(|name| name.strip_prefix(MOUNT_PREFIX))
                .is_some_and(|rest| rest.starts_with(&short))
        })
    }

    fn desktop(&self, env: &UserEnv) -> Option<Desktop> {
        self.desktop_target(env)
            .filter(|desktop| desktop.path.is_file())
    }

    fn desktop_target(&self, env: &UserEnv) -> Option<Desktop> {
        Self::app_image(env).map(|path| Desktop {
            name: DESKTOP_NAME,
            path,
        })
    }

    fn desktop_version(&self, _desktop: &Desktop) -> Option<String> {
        None
    }

    fn carried_floti(&self, _desktop: &Desktop) -> Option<PathBuf> {
        // Inside the image; the app runs install-self when it starts.
        None
    }

    fn is_desktop_process(&self, desktop: &Desktop, row: &ProcessRow) -> bool {
        // The AppImage's own runtime runs as long as the app it mounted, and
        // the kernel names it after the file (cut to 15 bytes).
        let name = file_name(&desktop.path);
        let short: String = name.chars().take(15).collect();
        if row.name == name || row.name == short {
            return true;
        }
        row.command_line.as_deref().is_some_and(|command_line| {
            let program = command_line.split(' ').next().unwrap_or_default();
            Path::new(program) == desktop.path
        })
    }

    fn opener(&self, desktop: &Desktop) -> (PathBuf, Vec<String>) {
        (desktop.path.clone(), Vec::new())
    }

    fn display_env(&self, pid: u32) -> Vec<(String, String)> {
        // The user's own process, so its environment is readable to them.
        std::fs::read(format!("/proc/{pid}/environ"))
            .map(|environ| super::display_vars(&environ))
            .unwrap_or_default()
    }

    fn no_display(&self, env: &UserEnv) -> Option<String> {
        (env.var("DISPLAY").is_none() && env.var("WAYLAND_DISPLAY").is_none()).then(|| {
            "this session has no display (neither DISPLAY nor WAYLAND_DISPLAY is set)".to_owned()
        })
    }

    fn add_launcher(&self, env: &UserEnv, desktop: &Desktop) -> Result<Option<PathBuf>> {
        let share = Self::data_home(env).ok_or_else(|| {
            Error::Refused("No HOME or XDG_DATA_HOME names where menu entries go.".to_owned())
        })?;
        let icon = share
            .join("icons")
            .join("hicolor")
            .join(ICON_SIZE)
            .join("apps")
            .join(format!("{ENTRY_NAME}.png"));
        write_into_place(&icon, ICON)?;
        let entry = share
            .join("applications")
            .join(format!("{ENTRY_NAME}.desktop"));
        write_into_place(&entry, Self::entry_text(&desktop.path).as_bytes())?;
        Ok(Some(entry))
    }

    fn install_desktop(&self, desktop: &Desktop, release: &Path) -> Result<()> {
        if let Some(folder) = desktop.path.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|source| Error::path("make", folder, source))?;
        }
        let new = beside(&desktop.path, NEW_COPY);
        std::fs::copy(release, &new).map_err(|source| Error::path("write", &new, source))?;
        posix::rename_over(&desktop.path, &new).inspect_err(|_| {
            let _ = std::fs::remove_file(&new);
        })
    }
}

#[cfg(any(test, feature = "testkit"))]
impl super::FakeDesktop for Native {
    fn fake_install(&self, apps: &Path, version: &str) {
        std::fs::create_dir_all(apps).expect("make the fake Applications folder");
        std::fs::write(
            apps.join(Self::app_image_name()),
            format!("installed {version}"),
        )
        .expect("write the fake AppImage");
    }

    fn fake_release(&self, _work: &Path, version: &str, _installer: &Path) -> (String, Vec<u8>) {
        (
            format!("{DESKTOP_NAME}-{version}-linux-x64.AppImage"),
            version.as_bytes().to_vec(),
        )
    }

    fn was_updated(&self, apps: &Path, version: &str, _record: &Path) -> bool {
        std::fs::read(apps.join(Self::app_image_name()))
            .is_ok_and(|bytes| bytes == version.as_bytes())
    }
}
