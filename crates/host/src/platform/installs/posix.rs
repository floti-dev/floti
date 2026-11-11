//! What the macOS and Linux installs share: where floti is installed on its
//! own, and putting a binary in place by renaming it over the old one,
//! after making it executable.

use std::path::Path;

use crate::platform::folders::UserEnv;
use crate::{Error, Result};

/// Makes `path` executable by everyone and writable by its owner (`0755`).
/// Compiled on every OS for the tests of the files that use it; it is
/// `Native` only on macOS and Linux.
pub(super) fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .map_err(|source| Error::path("make executable", path, source))
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(path)
            .map(|_| ())
            .map_err(|source| Error::path("make executable", path, source))
    }
}

/// Puts `new`, beside `running`, in its place: executable, then renamed over
/// it, so the name never stands empty and the running process keeps its own
/// copy.
pub(super) fn rename_over(running: &Path, new: &Path) -> Result<()> {
    make_executable(new)?;
    std::fs::rename(new, running).map_err(|source| Error::path("replace", running, source))
}

/// Where `scripts/install/install.sh` puts floti: `~/.local/bin/floti`.
pub(super) fn standalone_binary(env: &UserEnv) -> Option<std::path::PathBuf> {
    env.home
        .as_ref()
        .map(|home| home.join(".local").join("bin").join("floti"))
}
