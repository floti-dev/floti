//! Linux: the binary is renamed over; the desktop app is the AppImage at
//! `~/Applications/Helm.AppImage`, written beside and renamed into place in
//! the same way.
//!
//! `~/Applications` is where AppImage's own integration tools look for
//! AppImages. It is outside Floti's data folder, which `floti uninstall`
//! keeps. An AppImage carries no version floti can read without running
//! it, so its version is unknown and an update always installs it.

use std::path::{Path, PathBuf};

use super::{Desktop, Installs, NEW_COPY, beside, desktop_dir_override, file_name, posix};
use crate::platform::folders::UserEnv;
use crate::platform::process_table::ProcessRow;
use crate::{Error, Result};

/// What the desktop app is called: `productName` in `electron-builder.yml`.
pub const DESKTOP_NAME: &str = "Helm";

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
}

impl Installs for Native {
    fn replace_binary(&self, running: &Path, new: &Path) -> Result<()> {
        posix::rename_over(running, new)
    }

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

    fn standalone_binary(&self, env: &UserEnv) -> Option<PathBuf> {
        posix::standalone_binary(env)
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

    fn installing_desktop_stops_host(&self) -> bool {
        // The host runs on from the old image's mount until it next stops.
        false
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
            format!("{DESKTOP_NAME}-{version}-x86_64.AppImage"),
            version.as_bytes().to_vec(),
        )
    }

    fn was_updated(&self, apps: &Path, version: &str, _record: &Path) -> bool {
        std::fs::read(apps.join(Self::app_image_name()))
            .is_ok_and(|bytes| bytes == version.as_bytes())
    }
}
