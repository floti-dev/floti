//! Linux: the binary is renamed over; the desktop app is the AppImage at
//! `~/Applications/Floti.AppImage`, renamed over in the same way.
//!
//! `~/Applications` is where AppImage's own integration tools look for
//! AppImages, and `floti add desktop` installs it there. It is outside
//! Floti's data folder, which `floti uninstall` keeps.

use std::path::{Path, PathBuf};

use super::{Desktop, Installs, beside, desktop_dir_override, file_name, posix};
use crate::platform::folders::UserEnv;
use crate::platform::process_table::ProcessRow;
use crate::{Error, Result};

/// What the desktop app is called.
pub const DESKTOP_NAME: &str = "Floti";

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

    fn desktop(&self, env: &UserEnv) -> Option<Desktop> {
        Self::app_image(env)
            .filter(|path| path.is_file())
            .map(|path| Desktop {
                name: DESKTOP_NAME,
                path,
            })
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

    fn replace_desktop(&self, desktop: &Desktop, release: &Path) -> Result<()> {
        let new = beside(&desktop.path, "floti-update");
        std::fs::copy(release, &new).map_err(|source| Error::path("write", &new, source))?;
        posix::rename_over(&desktop.path, &new).inspect_err(|_| {
            let _ = std::fs::remove_file(&new);
        })
    }
}

#[cfg(any(test, feature = "testkit"))]
impl super::FakeDesktop for Native {
    fn fake_install(&self, apps: &Path) {
        std::fs::create_dir_all(apps).expect("make the fake Applications folder");
        std::fs::write(apps.join(Self::app_image_name()), b"installed")
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
