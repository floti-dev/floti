//! The `PATH` a new terminal of this user's gets, as the OS keeps it, for
//! `floti doctor`: whether floti is on it, and the one line that puts its
//! folder there. floti is on it when its own folder is, or when a `floti` in
//! one of its folders is this one by another name, such as a Homebrew link
//! into its Cellar. It is read without changing or running anything, and this
//! process's own `PATH` plays no part: a folder on it only because the
//! process that started floti put it there is not on the user's `PATH`.
//!
//! - Windows: the two `Path` values a new process's environment is built
//!   from, the system's (`HKLM\SYSTEM\CurrentControlSet\Control\Session
//!   Manager\Environment`) and the user's (`HKCU\Environment`), read from the
//!   registry with each `%VARIABLE%` expanded from this process's
//!   environment. The fix is a PowerShell line that appends the folder to
//!   the user's value as written, keeping it an expandable string, so no
//!   `%VARIABLE%` in it is frozen.
//! - macOS and Linux: a login shell's `PATH` is what its startup files make
//!   it, and running them to learn it would do whatever else they do, so
//!   they are read as text and never run. A folder is on it when it is one a
//!   login shell starts with (the standard folders, and the system's own
//!   list: `/etc/paths` and `/etc/paths.d` on macOS, `/etc/environment` on
//!   Linux), or when a line that sets `PATH` in a startup file the user's
//!   shell (`$SHELL`) reads names it: for bash the first of
//!   `~/.bash_profile`, `~/.bash_login` and `~/.profile` that exists, and
//!   `~/.bashrc`; for zsh `~/.zshenv`, `~/.zprofile`, `~/.zshrc` and
//!   `~/.zlogin`; for fish `~/.config/fish/config.fish`; for any other
//!   `~/.profile`. The fix appends a line to the first of those.
//!
//! Each OS's rule is plain text arithmetic over what was read, so all three
//! are compiled and tested on every OS; only reading the Windows registry is
//! Windows' own, and the build picks one as [`Native`].

pub mod linux;
pub mod macos;
mod posix;
pub mod windows;

#[cfg(target_os = "linux")]
pub use linux::Native;
#[cfg(target_os = "macos")]
pub use macos::Native;
#[cfg(target_os = "windows")]
pub use windows::Native;

use std::path::Path;

use super::folders::UserEnv;

#[cfg(test)]
mod tests;

/// One OS's way to keep a user's `PATH`.
pub trait UserPath {
    /// Whether `floti`, this program's own path, is on the `PATH` a new
    /// terminal of the user `env` describes gets, or why that could not be
    /// told.
    fn has(&self, env: &UserEnv, floti: &Path) -> Result<bool, String>;

    /// The one line, typed in the user's own shell, that puts `folder` on
    /// that `PATH` for every terminal opened after it.
    fn fix(&self, env: &UserEnv, folder: &Path) -> String;

    /// Puts `folder` on that `PATH` for every terminal and login started
    /// after it, as [`UserPath::fix`] does, and answers whether it changed
    /// anything: on Windows, the desktop installer's step; on macOS and
    /// Linux nothing is changed, and `floti doctor` gives the line.
    fn add(&self, env: &UserEnv, folder: &Path) -> Result<bool, String>;
}
