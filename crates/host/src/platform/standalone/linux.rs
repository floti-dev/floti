//! Linux: `~/.local/bin`, replaced by a rename over the old file.

use std::io;
use std::path::{Path, PathBuf};

use super::{Standalone, posix};
use crate::platform::folders::UserEnv;

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
}
