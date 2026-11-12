//! Windows: a held file is refused with a sharing violation, or, for a file
//! being scanned or an image a just-exited program still maps, access
//! denied.

use std::io;

use super::HeldFiles;

/// The Windows classification. Compiled on every OS, so its tests run
/// everywhere; it reads only the error's code.
#[derive(Debug, Clone, Copy, Default)]
pub struct Native;

/// `ERROR_ACCESS_DENIED`: what Windows also gives for a file being scanned,
/// or the image of a program that has just exited.
pub const ERROR_ACCESS_DENIED: i32 = 5;
/// `ERROR_SHARING_VIOLATION`: another process has the file open.
pub const ERROR_SHARING_VIOLATION: i32 = 32;

impl HeldFiles for Native {
    fn is_held(&self, err: &io::Error) -> bool {
        matches!(
            err.raw_os_error(),
            Some(ERROR_SHARING_VIOLATION | ERROR_ACCESS_DENIED)
        )
    }
}
