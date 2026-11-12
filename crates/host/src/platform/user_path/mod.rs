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
//!   `~/.zlogin`; for fish `~/.config/fish/config.fish`; for sh, dash, ash,
//!   ksh, mksh and any other `~/.profile`. The fix appends floti's line,
//!   which ends in `# added by floti`, to the first of those, and so does
//!   the install ([`UserPath::add`]), for every shell but one floti knows no
//!   startup file of (`$SHELL` not set, or tcsh and the like), which is
//!   given the fix to run instead. `floti uninstall` takes out that line and
//!   no other.
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

use std::path::{Path, PathBuf};

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
    /// after it, as [`UserPath::fix`] does, and answers what it changed: on
    /// Windows the user's `Path` value; on macOS and Linux the startup file
    /// the fix names, which gets floti's line appended once.
    fn add(&self, env: &UserEnv, folder: &Path) -> Result<Added, String>;

    /// Takes the entry [`UserPath::add`] puts there for `folder` off that
    /// `PATH`, for `floti uninstall`, and answers whether there was one: on
    /// Windows each entry of the user's `Path` value that names the folder
    /// (as written or once its `%VARIABLE%`s are expanded, without case or a
    /// trailing separator) goes, and the rest stay as written, still an
    /// expandable string; the value goes when nothing is left in it. On
    /// macOS and Linux floti's line goes from each startup file of a shell
    /// floti knows, and every line the user wrote stays.
    fn remove(&self, env: &UserEnv, folder: &Path) -> Result<bool, String>;
}

/// What [`UserPath::add`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    /// The folder was put on the `PATH`: in the user's `Path` value on
    /// Windows (`file` is `None`), and in the startup file `file` on macOS
    /// and Linux.
    Put { file: Option<PathBuf> },
    /// Nothing was changed: floti's line is there already.
    Nothing,
    /// Nothing was changed: floti knows no startup file the user's shell
    /// reads, and `line` is the command that puts the folder there.
    ByHand { line: String },
}

impl Added {
    /// The one line an install says of it, or `None` where nothing changed.
    pub fn sentence(&self, folder: &Path) -> Option<String> {
        let folder = folder.display();
        match self {
            Self::Put { file: None } => Some(format!(
                "Put {folder} on your PATH, for terminals and logins started from now on."
            )),
            Self::Put { file: Some(file) } => Some(format!(
                "Put {folder} on your PATH in {}, for terminals and logins started from now on.",
                file.display()
            )),
            Self::Nothing => None,
            Self::ByHand { line } => Some(format!(
                "{folder} is not on your PATH, and Floti does not know which startup file your shell reads, so it changed none; this puts it there: {line}"
            )),
        }
    }
}

/// Puts the folder of `floti`, this program's own path, on the `PATH` of
/// the user `env` describes through `path`, when it is not on it already.
pub fn put_on_path(path: &dyn UserPath, env: &UserEnv, floti: &Path) -> Result<Added, String> {
    let folder = floti
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .ok_or_else(|| format!("{} is not in a folder", floti.display()))?;
    if path.has(env, floti)? {
        return Ok(Added::Nothing);
    }
    path.add(env, folder)
}
