//! What macOS and Linux share: `~/.local/bin`, and a rename over the old
//! file, which a process running it keeps its own copy of.

use std::io;
use std::path::{Path, PathBuf};

use crate::platform::folders::UserEnv;

/// `~/.local/bin` for the user `env` describes.
pub(super) fn folder(env: &UserEnv) -> Option<PathBuf> {
    env.home
        .as_deref()
        .map(|home| home.join(".local").join("bin"))
}

/// Renames `new` over `target`.
pub(super) fn replace(target: &Path, new: &Path) -> io::Result<()> {
    std::fs::rename(new, target)
}
