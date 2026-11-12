//! macOS and Linux: a file another process has open can still be renamed and
//! unlinked, so no failure is a held file, and nothing is tried again.

use std::io;

use super::HeldFiles;

/// The macOS and Linux classification.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

impl HeldFiles for Native {
    fn is_held(&self, _err: &io::Error) -> bool {
        false
    }
}
