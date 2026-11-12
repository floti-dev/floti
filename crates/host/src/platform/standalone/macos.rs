//! macOS: `~/.local/bin`, replaced by a rename over the old file, with no
//! quarantine flag: a file copied out of an app opened from a downloaded dmg
//! keeps the `com.apple.quarantine` the dmg gave it, and Gatekeeper would
//! then refuse to run floti from `~/.local/bin`, where nothing opens it once
//! with Open Anyway.

use std::io;
use std::path::{Path, PathBuf};

use super::{Standalone, posix};
use crate::platform::folders::UserEnv;
use crate::platform::process_tree::Outliving;

/// The macOS place.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

/// The extended attribute Gatekeeper reads to quarantine a download.
pub const QUARANTINE: &str = "com.apple.quarantine";

impl Standalone for Native {
    fn folder(&self, env: &UserEnv) -> Option<PathBuf> {
        posix::folder(env)
    }

    fn binary(&self, env: &UserEnv) -> Option<PathBuf> {
        posix::binary(env)
    }

    fn replace(&self, target: &Path, new: &Path) -> io::Result<()> {
        strip_quarantine(new)?;
        posix::replace(target, new)
    }

    fn remove_asides(&self, _target: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    fn native_cpu(&self) -> &'static str {
        crate::version::cpu_name()
    }

    fn own_folders(&self, _env: &UserEnv) -> Vec<PathBuf> {
        Vec::new()
    }

    fn remove_running(&self, target: &Path) -> io::Result<Option<PathBuf>> {
        posix::remove_running(target)
    }

    fn delete_after_exit(
        &self,
        _env: &UserEnv,
        files: &[PathBuf],
        folders: &[PathBuf],
    ) -> io::Result<Outliving> {
        posix::delete_now(files, folders).map(|()| Outliving::Apart)
    }
}

/// Removes [`QUARANTINE`] from `path`; a file without it is as wanted.
/// Compiled on every OS for the tests of this file; it changes anything only
/// on macOS, the one OS with the attribute.
fn strip_quarantine(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let file = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::other(format!("{} has a NUL in it", path.display())))?;
        let name = CString::new(QUARANTINE).map_err(io::Error::other)?;
        // SAFETY: both are NUL-terminated C strings that outlive the call.
        let status = unsafe { libc::removexattr(file.as_ptr(), name.as_ptr(), 0) };
        if status == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ENOATTR) {
            Ok(())
        } else {
            Err(err)
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::metadata(path).map(|_| ())
    }
}
