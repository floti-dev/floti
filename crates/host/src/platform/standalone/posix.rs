//! What macOS and Linux share: `~/.local/bin`, and the new file made
//! executable and renamed over the old one, which a process running it
//! keeps its own copy of.

use std::io;
use std::path::{Path, PathBuf};

use crate::platform::folders::UserEnv;

/// `~/.local/bin` for the user `env` describes.
pub(super) fn folder(env: &UserEnv) -> Option<PathBuf> {
    env.home
        .as_deref()
        .map(|home| home.join(".local").join("bin"))
}

/// `~/.local/bin/floti`, where install.sh puts it.
pub(super) fn binary(env: &UserEnv) -> Option<PathBuf> {
    folder(env).map(|folder| folder.join("floti"))
}

/// Makes `new` executable (`0755`) and renames it over `target`, so the
/// name never stands empty.
pub(super) fn replace(target: &Path, new: &Path) -> io::Result<()> {
    make_executable(new)?;
    std::fs::rename(new, target)
}

/// Makes `path` executable by everyone and writable by its owner (`0755`).
/// Compiled on every OS for the tests of the files that use it; it is
/// `Native` only on macOS and Linux.
fn make_executable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(path).map(|_| ())
    }
}
