//! macOS: the binary is renamed over; the desktop app is `Helm.app`,
//! replaced by the one in the release's zip.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{Desktop, Installs, beside, desktop_dir_override, posix};
use crate::platform::folders::UserEnv;
use crate::platform::links::{self, remove_tree};
use crate::platform::process_table::ProcessRow;
use crate::{Error, Result};

/// What the desktop app is called: `productName` in `electron-builder.yml`.
pub const DESKTOP_NAME: &str = "Helm";

/// macOS's own archive tool, which keeps a bundle's links, modes and
/// signature intact where a plain unzip would not.
const DITTO: &str = "/usr/bin/ditto";

/// The Applications folder every user's apps may be in.
const SYSTEM_APPLICATIONS: &str = "/Applications";

pub struct Native;

impl Native {
    fn bundle_name() -> String {
        format!("{DESKTOP_NAME}.app")
    }

    /// The folders looked in, in order: the user's own Applications folder,
    /// then the system's; or only the one `FLOTI_DESKTOP_DIR` names.
    fn folders(env: &UserEnv) -> Vec<PathBuf> {
        if let Some(only) = desktop_dir_override(env) {
            return vec![only];
        }
        let mut folders = Vec::new();
        if let Some(home) = &env.home {
            folders.push(home.join("Applications"));
        }
        folders.push(PathBuf::from(SYSTEM_APPLICATIONS));
        folders
    }

    fn remove(path: &Path) -> Result<()> {
        remove_tree(&links::Native, path)
    }
}

impl Installs for Native {
    fn replace_binary(&self, running: &Path, new: &Path) -> Result<()> {
        posix::rename_over(running, new)
    }

    fn desktop(&self, env: &UserEnv) -> Option<Desktop> {
        Self::folders(env)
            .into_iter()
            .map(|folder| folder.join(Self::bundle_name()))
            .find(|app| app.join("Contents").join("Info.plist").is_file())
            .map(|path| Desktop {
                name: DESKTOP_NAME,
                path,
            })
    }

    fn is_desktop_process(&self, desktop: &Desktop, row: &ProcessRow) -> bool {
        match &row.command_line {
            Some(command_line) => {
                let contents = format!("{}/Contents/", desktop.path.to_string_lossy());
                command_line.starts_with(&contents)
            }
            None => row.name == DESKTOP_NAME,
        }
    }

    fn replace_desktop(&self, desktop: &Desktop, release: &Path) -> Result<()> {
        let unpacked = beside(&desktop.path, "floti-update");
        let old = beside(&desktop.path, "floti-old");
        Self::remove(&unpacked)?;
        Self::remove(&old)?;
        let replaced = (|| {
            let status = Command::new(DITTO)
                .args(["-x", "-k", "--noqtn"])
                .arg(release)
                .arg(&unpacked)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|source| Error::path("run", DITTO, source))?;
            if !status.success() {
                return Err(Error::Refused(format!(
                    "ditto could not unpack the {DESKTOP_NAME} release ({status})."
                )));
            }
            let new = unpacked.join(Self::bundle_name());
            if !new.join("Contents").join("Info.plist").is_file() {
                return Err(Error::Refused(format!(
                    "The {DESKTOP_NAME} release holds no {}.",
                    Self::bundle_name()
                )));
            }
            std::fs::rename(&desktop.path, &old)
                .map_err(|source| Error::path("move aside", &desktop.path, source))?;
            if let Err(source) = std::fs::rename(&new, &desktop.path) {
                let _ = std::fs::rename(&old, &desktop.path);
                return Err(Error::path("replace", &desktop.path, source));
            }
            Ok(())
        })();
        let _ = Self::remove(&unpacked);
        let _ = Self::remove(&old);
        replaced
    }
}

#[cfg(any(test, feature = "testkit"))]
impl super::FakeDesktop for Native {
    fn fake_install(&self, apps: &Path) {
        let contents = apps.join(Self::bundle_name()).join("Contents");
        std::fs::create_dir_all(&contents).expect("make the fake app");
        std::fs::write(contents.join("Info.plist"), "installed").expect("write the fake app");
    }

    fn fake_release(&self, work: &Path, version: &str, _installer: &Path) -> (String, Vec<u8>) {
        let contents = work.join(Self::bundle_name()).join("Contents");
        std::fs::create_dir_all(&contents).expect("make the fake release's app");
        std::fs::write(contents.join("Info.plist"), version).expect("write the fake release");
        let zip = work.join("release.zip");
        let status = Command::new(DITTO)
            .args(["-c", "-k", "--keepParent"])
            .arg(work.join(Self::bundle_name()))
            .arg(&zip)
            .status()
            .expect("run ditto");
        assert!(status.success(), "ditto made no zip: {status}");
        let file = format!("{DESKTOP_NAME}-{version}-arm64-mac.zip");
        (file, std::fs::read(&zip).expect("read the fake release"))
    }

    fn was_updated(&self, apps: &Path, version: &str, _record: &Path) -> bool {
        std::fs::read_to_string(
            apps.join(Self::bundle_name())
                .join("Contents")
                .join("Info.plist"),
        )
        .is_ok_and(|text| text == version)
    }
}
