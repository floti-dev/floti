//! A file something holds for a moment: an antivirus scanning a just-written
//! `.exe`, or a program that has just exited and whose image Windows has not
//! let go of yet. Windows refuses to rename or delete such a file until it
//! is let go; macOS and Linux never refuse that way.
//!
//! [`retry`] tries an operation again while it fails that way, with a pause
//! that doubles each time, for a few seconds in all, then answers the
//! operation's own error. [`HeldFiles`] says which failures are a held file,
//! one implementation per OS. `Standalone::replace` renames through it, and
//! `floti uninstall` removes the desktop app's folder through
//! [`remove_tree`], once the app's own uninstaller has exited.

pub mod posix;
pub mod windows;

#[cfg(unix)]
pub use posix::Native;
#[cfg(windows)]
pub use windows::Native;

#[cfg(test)]
mod tests;

use std::io;
use std::path::Path;
use std::time::Duration;

use crate::platform::links::{self, FolderLinks};
use crate::{Error, Result};

/// Which failures are a file something holds for now.
pub trait HeldFiles {
    /// Whether `err` is the OS refusing because something holds the file for
    /// now, so that trying again in a moment may work.
    fn is_held(&self, err: &io::Error) -> bool;
}

/// How long one operation keeps trying while something holds the file, in
/// all.
pub const HELD_PATIENCE: Duration = Duration::from_secs(5);
/// The first pause before trying again; each pause doubles.
pub const FIRST_PAUSE: Duration = Duration::from_millis(25);
/// The longest single pause between tries.
pub const LONGEST_PAUSE: Duration = Duration::from_millis(800);

/// Runs `op`, and while it fails with an error `held` says is a held file,
/// pauses through `sleep` and runs it again, each pause twice the last (up
/// to [`LONGEST_PAUSE`]), up to [`HELD_PATIENCE`] of pauses in all. Then, or
/// on any other failure, it answers the operation's own error, so a file
/// held for good costs the patience before the error a single try gives.
pub fn retry<T, E>(
    held: &dyn Fn(&E) -> bool,
    op: &mut dyn FnMut() -> std::result::Result<T, E>,
    sleep: &mut dyn FnMut(Duration),
) -> std::result::Result<T, E> {
    let mut waited = Duration::ZERO;
    let mut pause = FIRST_PAUSE;
    loop {
        match op() {
            Err(err) if held(&err) && waited < HELD_PATIENCE => {
                let this_pause = pause.min(HELD_PATIENCE - waited);
                sleep(this_pause);
                waited += this_pause;
                pause = (pause * 2).min(LONGEST_PAUSE);
            }
            result => return result,
        }
    }
}

/// [`links::remove_tree`] on `root`, tried again while a file in it is held
/// on this OS. Each try starts from what the last one left, since a tree
/// already removed in part is removed the same way.
pub fn remove_tree(folder_links: &dyn FolderLinks, root: &Path) -> Result<()> {
    remove_held(
        &Native,
        &mut || links::remove_tree(folder_links, root),
        &mut std::thread::sleep,
    )
}

/// Runs the removal `remove`, tried again through [`retry`] while it fails
/// on a file `held` says is held, pausing through `sleep`.
pub fn remove_held(
    held: &dyn HeldFiles,
    remove: &mut dyn FnMut() -> Result<()>,
    sleep: &mut dyn FnMut(Duration),
) -> Result<()> {
    retry(
        &|err: &Error| io_source(err).is_some_and(|source| held.is_held(source)),
        remove,
        sleep,
    )
}

/// The OS error behind `err`, where it is one.
fn io_source(err: &Error) -> Option<&io::Error> {
    match err {
        Error::Io { source, .. } | Error::Path { source, .. } => Some(source),
        _ => None,
    }
}
