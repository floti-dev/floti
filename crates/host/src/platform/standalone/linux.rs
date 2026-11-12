//! Linux: `~/.local/bin`, replaced by a rename over the old file.

use std::io;
use std::path::{Path, PathBuf};

use super::{Standalone, posix};
use crate::platform::folders::UserEnv;
use crate::platform::process_tree::Outliving;

/// The Linux place.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

impl Standalone for Native {
    fn folder(&self, env: &UserEnv) -> Option<PathBuf> {
        posix::folder(env)
    }

    fn binary(&self, env: &UserEnv) -> Option<PathBuf> {
        posix::binary(env)
    }

    fn replace(&self, target: &Path, new: &Path) -> io::Result<()> {
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
