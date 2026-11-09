//! Folder links on macOS and Linux: symlinks, which any user may make.

use std::io;
use std::path::Path;

use super::FolderLinks;
use crate::{Error, Result};

/// The implementation macOS and Linux share.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

impl FolderLinks for Native {
    fn link_folder(&self, target: &Path, link: &Path) -> Result<()> {
        std::os::unix::fs::symlink(target, link)
            .map_err(|source| Error::path("link a folder at", link, source))
    }

    /// `unlink` on a symlink removes the link, whatever it points to.
    fn unlink_folder(&self, link: &Path) -> io::Result<()> {
        std::fs::remove_file(link)
    }
}

impl Native {
    /// Makes `link`, which must not exist, a symlink to the file `target`.
    /// Floti makes no file links of its own; this is the link a user's own
    /// tools make, such as a dotfiles manager's, for what reads and writes
    /// through one.
    pub fn link_file(&self, target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }
}
