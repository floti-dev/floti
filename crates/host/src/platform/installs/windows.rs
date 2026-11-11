//! Windows: the running `floti.exe` is renamed aside and the new one moved
//! into its name; the desktop app is the per-user NSIS install, updated by
//! the release's own installer run silently.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{Desktop, Installs, desktop_dir_override, file_name};
use crate::platform::folders::UserEnv;
use crate::platform::process_table::ProcessRow;
use crate::{Error, Result};

/// What the desktop app is called, and so its install folder, its program
/// and its uninstaller: `productName` and `extraMetadata.name` in
/// `electron-builder.yml`.
pub const DESKTOP_NAME: &str = "Helm";

pub struct Native;

impl Native {
    fn program(install: &Path) -> PathBuf {
        install.join(format!("{DESKTOP_NAME}.exe"))
    }

    fn uninstaller(install: &Path) -> PathBuf {
        install.join(format!("Uninstall {DESKTOP_NAME}.exe"))
    }

    /// Where a running exe goes aside: `<stem>.old.exe` beside it, or, when
    /// one from an earlier update is still running, a name of this
    /// process's own.
    fn aside(running: &Path) -> PathBuf {
        let stem = running
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = running
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        let old = running.with_file_name(format!("{stem}.old{extension}"));
        match std::fs::remove_file(&old) {
            Ok(()) => old,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => old,
            Err(_) => {
                running.with_file_name(format!("{stem}.old-{}{extension}", std::process::id()))
            }
        }
    }
}

impl Installs for Native {
    fn replace_binary(&self, running: &Path, new: &Path) -> Result<()> {
        let old = Self::aside(running);
        std::fs::rename(running, &old)
            .map_err(|source| Error::path("move aside", running, source))?;
        if let Err(source) = std::fs::rename(new, running) {
            let _ = std::fs::rename(&old, running);
            return Err(Error::path("replace", running, source));
        }
        // Refused while this process runs from it; the next update removes it.
        let _ = std::fs::remove_file(&old);
        Ok(())
    }

    fn desktop(&self, env: &UserEnv) -> Option<Desktop> {
        let programs = desktop_dir_override(env).or_else(|| {
            env.var_any_case("LOCALAPPDATA")
                .map(|local| PathBuf::from(local).join("Programs"))
        })?;
        let install = programs.join(DESKTOP_NAME);
        // A portable copy has no uninstaller and is not an install.
        (Self::program(&install).is_file() && Self::uninstaller(&install).is_file()).then_some(
            Desktop {
                name: DESKTOP_NAME,
                path: install,
            },
        )
    }

    fn is_desktop_process(&self, desktop: &Desktop, row: &ProcessRow) -> bool {
        let program = file_name(&Self::program(&desktop.path));
        if !row.name.eq_ignore_ascii_case(&program) {
            return false;
        }
        let Some(command_line) = &row.command_line else {
            return true;
        };
        let wanted = Self::program(&desktop.path)
            .to_string_lossy()
            .to_lowercase();
        command_line.to_lowercase().contains(&wanted)
    }

    fn replace_desktop(&self, desktop: &Desktop, release: &Path) -> Result<()> {
        let status = Command::new(release)
            .arg("/S")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|source| Error::path("run", release, source))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Refused(format!(
                "The {} installer {} ended with {status}.",
                desktop.name,
                file_name(release)
            )))
        }
    }
}

#[cfg(any(test, feature = "testkit"))]
impl super::FakeDesktop for Native {
    fn fake_install(&self, apps: &Path) {
        let install = apps.join(DESKTOP_NAME);
        std::fs::create_dir_all(&install).expect("make the fake install folder");
        std::fs::write(Self::program(&install), b"fake app").expect("write the fake app");
        std::fs::write(Self::uninstaller(&install), b"fake uninstaller")
            .expect("write the fake uninstaller");
    }

    fn fake_release(&self, _work: &Path, version: &str, installer: &Path) -> (String, Vec<u8>) {
        (
            format!("{DESKTOP_NAME}-{version}-setup.exe"),
            std::fs::read(installer).expect("read the fake installer"),
        )
    }

    fn was_updated(&self, _apps: &Path, _version: &str, record: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(record) else {
            return false;
        };
        text.lines().any(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .is_ok_and(|run| run["args"] == serde_json::json!(["/S"]))
        })
    }
}
