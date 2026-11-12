//! macOS: the standard folders, `/etc/paths` and `/etc/paths.d`, which
//! `path_helper` builds every login shell's `PATH` from, then the user's
//! startup files.

use std::path::Path;

use super::UserPath;
use super::posix::{self, SystemPath};
use crate::platform::folders::UserEnv;

/// The macOS user `PATH`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

/// What a login shell's `PATH` starts with on macOS.
pub(super) const SYSTEM: SystemPath = SystemPath {
    standard: &["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"],
    listed_in: &["/etc/paths"],
    listed_in_folders: &["/etc/paths.d"],
    assigned_in: &[],
};

impl UserPath for Native {
    fn has(&self, env: &UserEnv, floti: &Path) -> Result<bool, String> {
        let home = env.require_home().map_err(|err| err.to_string())?;
        posix::has(&SYSTEM, Path::new("/"), env, home, floti)
    }

    fn fix(&self, env: &UserEnv, folder: &Path) -> String {
        posix::fix(env, folder)
    }
}
