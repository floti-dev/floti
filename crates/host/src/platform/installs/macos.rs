//! macOS: the binary is renamed over; the desktop app is `Floti.app`,
//! replaced by, or installed fresh in `~/Applications` as, the one in the
//! release's zip. A `Helm.app` from before the rename is found too, and an
//! update puts `Floti.app` beside it and removes it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{
    Desktop, DesktopRemoval, Installs, NEW_COPY, OLD_COPY, beside, desktop_dir_override,
    plist_string,
};
use crate::platform::folders::UserEnv;
use crate::platform::links::{self, remove_tree};
use crate::platform::process_table::ProcessRow;
use crate::platform::startup::register::{NativeSystem, SignInSystem, gui_domain};
use crate::{Error, Result};

/// What the desktop app is called: `productName` in `electron-builder.yml`.
pub const DESKTOP_NAME: &str = "Floti";

/// What the app was called before it was Floti, and so its bundle's name.
pub const HELM_NAME: &str = "Helm";

/// macOS's own archive tool, which keeps a bundle's links, modes and
/// signature intact where a plain unzip would not.
const DITTO: &str = "/usr/bin/ditto";

/// The Applications folder every user's apps may be in.
const SYSTEM_APPLICATIONS: &str = "/Applications";

pub struct Native;

impl Native {
    fn bundle_name(name: &str) -> String {
        format!("{name}.app")
    }

    /// The folders looked in, in order: the user's own Applications folder,
    /// then the system's; or, in a build with the testkit, only the one
    /// `FLOTI_DESKTOP_DIR` names.
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

    fn info_plist(app: &Path) -> PathBuf {
        app.join("Contents").join("Info.plist")
    }

    /// Moves `new` to `target`, first moving an app there aside to `old`
    /// and back again when the move fails.
    pub(super) fn swap_in(new: &Path, target: &Path, old: &Path) -> Result<()> {
        let replacing = std::fs::symlink_metadata(target).is_ok();
        if replacing {
            std::fs::rename(target, old)
                .map_err(|source| Error::path("move aside", target, source))?;
        }
        if let Err(source) = std::fs::rename(new, target) {
            if replacing {
                let _ = std::fs::rename(old, target);
            }
            return Err(Error::path("install", target, source));
        }
        Ok(())
    }
}

impl Installs for Native {
    fn bundles(&self, desktop: &Desktop, running: &Path) -> bool {
        running != desktop.path && running.starts_with(&desktop.path)
    }

    fn desktop(&self, env: &UserEnv) -> Option<Desktop> {
        // Floti's in any of the folders first, then one from before the
        // rename.
        [DESKTOP_NAME, HELM_NAME].into_iter().find_map(|name| {
            Self::folders(env)
                .into_iter()
                .map(|folder| folder.join(Self::bundle_name(name)))
                .find(|app| Self::info_plist(app).is_file())
                .map(|path| Desktop { name, path })
        })
    }

    fn desktop_target(&self, env: &UserEnv) -> Option<Desktop> {
        // A new install goes in the user's own Applications folder, which
        // needs no admin rights.
        self.desktop(env).or_else(|| {
            Self::folders(env).into_iter().next().map(|folder| Desktop {
                name: DESKTOP_NAME,
                path: folder.join(Self::bundle_name(DESKTOP_NAME)),
            })
        })
    }

    fn desktop_version(&self, desktop: &Desktop) -> Option<String> {
        let plist = std::fs::read_to_string(Self::info_plist(&desktop.path)).ok()?;
        plist_string(&plist, "CFBundleShortVersionString")
    }

    fn carried_floti(&self, desktop: &Desktop) -> Option<PathBuf> {
        Some(
            desktop
                .path
                .join("Contents")
                .join("Resources")
                .join("floti"),
        )
    }

    fn is_desktop_process(&self, desktop: &Desktop, row: &ProcessRow) -> bool {
        match &row.command_line {
            // The app and its helpers; not the host, its keeper or the Node
            // runtime it runs plugins on, which run from Contents/Resources
            // and outlive the app.
            Some(command_line) => ["MacOS", "Frameworks"].iter().any(|folder| {
                let inside = format!("{}/Contents/{folder}/", desktop.path.to_string_lossy());
                command_line.starts_with(&inside)
            }),
            None => row.name == desktop.name,
        }
    }

    fn removal(&self, env: &UserEnv) -> Option<DesktopRemoval> {
        self.desktop(env).map(|desktop| DesktopRemoval {
            remove: vec![desktop.path.clone()],
            shown: desktop.path,
            uninstaller: None,
            gone_after: None,
        })
    }

    fn opener(&self, desktop: &Desktop) -> (PathBuf, Vec<String>) {
        (
            PathBuf::from("/usr/bin/open"),
            vec![desktop.path.to_string_lossy().into_owned()],
        )
    }

    fn install_desktop(&self, desktop: &Desktop, release: &Path) -> Result<Desktop> {
        // In the folder the app is in, under its own name: `Floti.app` beside
        // a `Helm.app` from before the rename, which `remove_replaced` takes
        // away once it is in.
        let installed = Desktop {
            name: DESKTOP_NAME,
            path: desktop
                .path
                .with_file_name(Self::bundle_name(DESKTOP_NAME)),
        };
        let unpacked = beside(&installed.path, NEW_COPY);
        let old = beside(&installed.path, OLD_COPY);
        if let Some(folder) = installed.path.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|source| Error::path("make", folder, source))?;
        }
        Self::remove(&unpacked)?;
        Self::remove(&old)?;
        let swapped = (|| {
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
            let new = unpacked.join(Self::bundle_name(DESKTOP_NAME));
            if !Self::info_plist(&new).is_file() {
                return Err(Error::Refused(format!(
                    "The {DESKTOP_NAME} release holds no {}.",
                    Self::bundle_name(DESKTOP_NAME)
                )));
            }
            Self::swap_in(&new, &installed.path, &old)
        })();
        let _ = Self::remove(&unpacked);
        let _ = Self::remove(&old);
        swapped?;
        Ok(installed)
    }

    fn remove_replaced(&self, replaced: &Desktop, installed: &Desktop) -> Result<()> {
        if replaced.path == installed.path {
            return Ok(());
        }
        Self::remove(&replaced.path)
    }

    fn no_display(&self, _env: &UserEnv) -> Option<String> {
        no_display_with(&NativeSystem)
    }
}

/// Why this Mac has no display to open the app on, asked through `system`:
/// nobody is signed in at its screen, so launchd has no GUI domain for this
/// user ([`gui_domain`], the way setup learns the host starts at the next
/// sign-in). `None` when the domain is there, or when the user's id or
/// launchd could not be asked.
pub fn no_display_with(system: &dyn SignInSystem) -> Option<String> {
    matches!(gui_domain(system), Ok(None)).then(|| NO_GUI_DOMAIN.to_owned())
}

/// What [`no_display_with`] says, as a clause.
pub const NO_GUI_DOMAIN: &str =
    "nobody is signed in at this Mac's screen (launchd has no GUI domain for this user)";

#[cfg(any(test, feature = "testkit"))]
impl super::FakeDesktop for Native {
    fn fake_install(&self, apps: &Path, version: &str) {
        let app = apps.join(Self::bundle_name(DESKTOP_NAME));
        std::fs::create_dir_all(app.join("Contents")).expect("make the fake app");
        std::fs::write(Self::info_plist(&app), fake_plist(version)).expect("write the fake app");
    }

    fn fake_release(&self, work: &Path, version: &str, installer: &Path) -> (String, Vec<u8>) {
        let app = work.join(Self::bundle_name(DESKTOP_NAME));
        let resources = app.join("Contents").join("Resources");
        std::fs::create_dir_all(&resources).expect("make the fake release's app");
        std::fs::write(Self::info_plist(&app), fake_plist(version))
            .expect("write the fake release");
        // The floti it carries is the fake program, which records being run.
        let carried = resources.join("floti");
        std::fs::copy(installer, &carried).expect("put the fake floti in the fake app");
        super::posix::make_executable(&carried).expect("make the fake floti executable");
        let zip = work.join("release.zip");
        let status = Command::new(DITTO)
            .args(["-c", "-k", "--keepParent"])
            .arg(work.join(Self::bundle_name(DESKTOP_NAME)))
            .arg(&zip)
            .status()
            .expect("run ditto");
        assert!(status.success(), "ditto made no zip: {status}");
        let file = format!("{DESKTOP_NAME}-{version}-arm64-mac.zip");
        (file, std::fs::read(&zip).expect("read the fake release"))
    }

    fn was_updated(&self, apps: &Path, version: &str, _record: &Path) -> bool {
        let desktop = Desktop {
            name: DESKTOP_NAME,
            path: apps.join(Self::bundle_name(DESKTOP_NAME)),
        };
        self.desktop_version(&desktop).as_deref() == Some(version)
    }
}

/// An app's `Info.plist`, as electron-builder writes it, cut to its version.
#[cfg(any(test, feature = "testkit"))]
pub fn fake_plist(version: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\t<key>CFBundleName</key>\n\t<string>{DESKTOP_NAME}</string>\n\t<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>\n</dict>\n</plist>\n"
    )
}
