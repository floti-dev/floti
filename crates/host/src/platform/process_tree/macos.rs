//! Ending a process tree on macOS, through its process group: the
//! implementation it shares with Linux.

pub use super::unix::{Native, OneProcess};

#[cfg(any(test, feature = "testkit"))]
pub use super::unix::descendants;

pub use super::unix::console_of;
