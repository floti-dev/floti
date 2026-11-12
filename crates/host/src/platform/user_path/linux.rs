//! Linux: the standard folders and `/etc/environment`, which `pam_env` sets
//! for every login, then the user's startup files.

use std::path::Path;

use super::UserPath;
use super::posix::{self, SystemPath};
use crate::platform::folders::UserEnv;

/// The Linux user `PATH`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

/// What a login shell's `PATH` starts with on Linux.
pub(super) const SYSTEM: SystemPath = SystemPath {
    standard: &[
        "/usr/local/sbin",
        "/usr/local/bin",
        "/usr/sbin",
        "/usr/bin",
        "/sbin",
        "/bin",
    ],
    listed_in: &[],
    listed_in_folders: &[],
    assigned_in: &["/etc/environment"],
};

impl UserPath for Native {
    fn has(&self, env: &UserEnv, folder: &Path) -> Result<bool, String> {
        let home = env.require_home().map_err(|err| err.to_string())?;
        posix::has(&SYSTEM, Path::new("/"), home, folder)
    }

    fn fix(&self, env: &UserEnv, folder: &Path) -> String {
        posix::fix(env, folder)
    }
}
