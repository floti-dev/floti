//! Folder links on Windows: directory junctions, which need no elevation and
//! no developer mode, as today's app makes them.

use std::io;
use std::path::Path;

use super::FolderLinks;
use crate::{Error, Result};

/// The Windows implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

impl FolderLinks for Native {
    fn link_folder(&self, target: &Path, link: &Path) -> Result<()> {
        junction::create(target, link)
            .map_err(|source| Error::path("link a folder at", link, source))
    }

    /// `RemoveDirectory` on a junction removes the junction, not the folder
    /// behind it.
    fn unlink_folder(&self, link: &Path) -> io::Result<()> {
        std::fs::remove_dir(link)
    }
}

impl Native {
    /// Makes `link`, which must not exist, a symlink to the file `target`.
    /// A file symlink needs developer mode or elevation on Windows, so Floti
    /// makes none of its own; this is the link a user's own tools make, such
    /// as a dotfiles manager's, for what reads and writes through one.
    pub fn link_file(&self, target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }
}
