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

/// Unlinks `target`; a process running it keeps its copy until it exits,
/// so nothing is left to delete later.
pub(super) fn remove_running(target: &Path) -> io::Result<Option<PathBuf>> {
    match std::fs::remove_file(target) {
        Ok(()) => Ok(None),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Deletes `files` and each empty folder of `folders` now: on macOS and
/// Linux no running process holds a file against its deletion.
pub(super) fn delete_now(files: &[PathBuf], folders: &[PathBuf]) -> io::Result<()> {
    for file in files {
        remove_running(file)?;
    }
    for folder in folders {
        let is_folder = std::fs::symlink_metadata(folder)
            .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink());
        if is_folder && std::fs::read_dir(folder)?.next().is_none() {
            std::fs::remove_dir(folder)?;
        }
    }
    Ok(())
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
