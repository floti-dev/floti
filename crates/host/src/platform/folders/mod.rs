//! Where Floti keeps its own data, its logs, the user's own files and the
//! socket's folder.
//!
//! What Floti keeps for itself lives in `%LOCALAPPDATA%\Floti` on Windows,
//! `~/Library/Application Support/Floti` on macOS and `$XDG_DATA_HOME/floti`
//! (default `~/.local/share/floti`) on Linux, with logs in
//! `~/.local/state/floti` there. What the user writes lives in
//! `~/.config/floti` on every OS, or `~/.config/helm` where Helm's is there
//! ([`config_folder`]).
//!
//! Each OS's layout is plain path arithmetic over a [`UserEnv`], so all three
//! are compiled and tested on every OS; the build picks one as [`Native`].
//! Resolving creates nothing on disk: whoever uses a folder creates it.
//!
//! [`resolve`] applies the overrides first. `FLOTI_DATA_DIR`,
//! `FLOTI_LOG_DIR`, `FLOTI_CONFIG_DIR` and `FLOTI_RUNTIME_DIR` each move one
//! folder, and the socket moves with the runtime folder. Tests use them so
//! nothing reaches the real folders.

pub mod linux;
pub mod macos;
pub mod windows;

#[cfg(target_os = "linux")]
pub use linux::Native;
#[cfg(target_os = "macos")]
pub use macos::Native;
#[cfg(target_os = "windows")]
pub use windows::Native;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::platform::links;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// Every folder the host uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    /// What Floti keeps for itself: `floti.db`, installed plugins, keys.
    pub data: PathBuf,
    /// The host's logs.
    pub logs: PathBuf,
    /// What the user writes: templates, plugins they are developing.
    pub config: PathBuf,
    /// The folder the socket lives in, where the socket is a file. Windows
    /// names a pipe instead and does not use it.
    pub runtime: PathBuf,
}

/// What resolving the folders may read: the home folder and the environment.
/// Taken as a value so a test resolves against a home of its own and never
/// the real one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserEnv {
    pub home: Option<PathBuf>,
    pub vars: BTreeMap<OsString, OsString>,
}

impl UserEnv {
    /// This process's home folder and environment.
    pub fn from_process() -> Self {
        Self {
            home: std::env::home_dir(),
            vars: std::env::vars_os().collect(),
        }
    }

    /// A variable, when it is set and not empty.
    pub fn var(&self, name: &str) -> Option<&OsString> {
        self.vars
            .get(&OsString::from(name))
            .filter(|value| !value.is_empty())
    }

    /// A variable whose name is matched ignoring case, when it is set and not
    /// empty. Windows names are case-insensitive and its environment block
    /// spells some its own way (`ComSpec`), so a Windows variable is looked
    /// up with this.
    pub fn var_any_case(&self, name: &str) -> Option<&OsString> {
        self.var(name).or_else(|| {
            self.vars
                .iter()
                .find(|(key, value)| {
                    !value.is_empty() && key.eq_ignore_ascii_case(OsString::from(name))
                })
                .map(|(_, value)| value)
        })
    }

    /// A variable that names a folder, when it is set to an absolute path.
    /// The XDG base directory rules say a relative one is ignored.
    pub fn absolute_var(&self, name: &str) -> Option<PathBuf> {
        self.var(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    }

    /// The home folder, or a refusal saying the host cannot place its folders
    /// without one.
    pub fn require_home(&self) -> Result<&Path> {
        self.home.as_deref().ok_or_else(|| {
            Error::Refused(
                "Floti cannot find your home folder, so it does not know where to keep its data."
                    .to_owned(),
            )
        })
    }
}

/// The OS's places for each of Floti's folders.
pub trait FolderLayout {
    /// The folders for the user `env` describes.
    fn resolve(&self, env: &UserEnv) -> Result<Folders>;
}

/// The user's own files, on every OS: `~/.config/helm` wherever it is
/// there, which Helm set up before the name changed and where its agents,
/// templates and plugins are, else `~/.config/floti`. The answer for a
/// computer does not change while Floti runs on it, so a host started by the
/// desktop, at sign-in or by hand reads one folder, even once one of them
/// has written `~/.config/floti`. The one-time move of `~/.config/helm` to
/// `~/.config/floti` ([`move_helm_config`]), which the first host to start
/// as Floti makes, is what switches a computer over. This is the one look at
/// the disk resolving makes, and it creates nothing.
pub fn config_folder(home: &Path) -> PathBuf {
    let config = home.join(".config");
    let helm = config.join(HELM_CONFIG_FOLDER);
    if helm.is_dir() {
        helm
    } else {
        config.join("floti")
    }
}

/// The folder in `~/.config` Helm kept the user's own files in.
pub const HELM_CONFIG_FOLDER: &str = "helm";

/// The folder in `~/.config` the user's own files are kept in.
pub const FLOTI_CONFIG_FOLDER: &str = "floti";

/// What [`move_helm_config`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigMove {
    /// There is no `~/.config/helm` to move.
    Nothing,
    /// `~/.config/helm` is now `~/.config/floti`, every file in it as it was.
    Moved { from: PathBuf, to: PathBuf },
    /// `~/.config/helm` stays where it is and is still the folder read, for
    /// the reason given, and the move is tried again at the next start.
    Kept {
        from: PathBuf,
        to: PathBuf,
        reason: String,
    },
}

impl ConfigMove {
    /// The host's log line for it, or `None` when there is nothing to say.
    pub fn log_line(&self) -> Option<String> {
        match self {
            Self::Nothing => None,
            Self::Moved { from, to } => Some(format!(
                "floti host: moved your files from {} to {}",
                from.display(),
                to.display()
            )),
            Self::Kept { from, to, reason } => Some(format!(
                "floti host: your files stay in {} and are read from there, because {reason}; moving them to {} is tried again at the next start",
                from.display(),
                to.display()
            )),
        }
    }

    /// The config folder to read once this move is done: Floti's when the
    /// files are there, else Helm's.
    pub fn config(&self) -> Option<&Path> {
        match self {
            Self::Nothing => None,
            Self::Moved { to, .. } => Some(to),
            Self::Kept { from, .. } => Some(from),
        }
    }
}

/// Moves the user's own files, once, from `~/.config/helm`, where Helm kept
/// them, to `~/.config/floti` in `home`: templates, themes, agents and the
/// plugins they are writing.
///
/// - No `~/.config/helm`: nothing.
/// - No `~/.config/floti`: the folder is renamed whole, every file and link
///   in it as it was. A `~/.config/helm` that is itself a link (a symlink,
///   or a junction on Windows) is renamed as the link, and what it points
///   to stays where it is.
/// - Both: each entry of Helm's is renamed into Floti's, and Helm's empty
///   folder removed, but only when no entry is in both, so no file of the
///   user's is ever replaced. Otherwise nothing moves and the reason names
///   the entries in both. A rename that fails partway is undone. A
///   `~/.config/helm` that is a link is never emptied this way: nothing
///   moves, and the reason says so.
///
/// Only a host that may write its config folder calls this: one started
/// with [`CONFIG_READ_ONLY_VAR`] moves nothing.
pub fn move_helm_config(home: &Path) -> ConfigMove {
    move_helm_config_with(home, |from, to| std::fs::rename(from, to))
}

/// The folders a host starting with `folders` reads once Helm's config
/// folder has been moved ([`move_helm_config`]), and what the move did. It
/// moves only for a host that may write its config folder (`read_only` is
/// [`CONFIG_READ_ONLY_VAR`] being set) whose config folder is Helm's own in
/// `home`, not one `FLOTI_CONFIG_DIR` moved: otherwise nothing moves and
/// `folders` is as it was.
pub fn after_helm_config_move(
    folders: Folders,
    home: Option<&Path>,
    read_only: bool,
) -> (Folders, ConfigMove) {
    let Some(home) = home else {
        return (folders, ConfigMove::Nothing);
    };
    if read_only || folders.config != home.join(".config").join(HELM_CONFIG_FOLDER) {
        return (folders, ConfigMove::Nothing);
    }
    let moved = move_helm_config(home);
    let folders = match moved.config() {
        Some(config) => Folders {
            config: config.to_path_buf(),
            ..folders
        },
        None => folders,
    };
    (folders, moved)
}

/// [`move_helm_config`], renaming with `rename`.
pub(crate) fn move_helm_config_with(
    home: &Path,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> ConfigMove {
    let config = home.join(".config");
    let from = config.join(HELM_CONFIG_FOLDER);
    let to = config.join(FLOTI_CONFIG_FOLDER);
    if !from.is_dir() {
        return ConfigMove::Nothing;
    }
    let kept = |reason: String| ConfigMove::Kept {
        from: from.clone(),
        to: to.clone(),
        reason,
    };
    if std::fs::symlink_metadata(&to).is_err() {
        return match rename(&from, &to) {
            Ok(()) => ConfigMove::Moved {
                from: from.clone(),
                to: to.clone(),
            },
            Err(err) => kept(format!("it could not be renamed: {err}")),
        };
    }
    if !to.is_dir() {
        return kept(format!("{} is not a folder", to.display()));
    }
    if links::is_link(&from) {
        return kept(format!(
            "it is a link and {} is there too, and moving what it points to would empty that folder",
            to.display()
        ));
    }
    let names = match std::fs::read_dir(&from) {
        Ok(entries) => {
            let mut names: Vec<_> = entries.flatten().map(|entry| entry.file_name()).collect();
            names.sort();
            names
        }
        Err(err) => return kept(format!("it could not be read: {err}")),
    };
    let in_both: Vec<String> = names
        .iter()
        .filter(|name| std::fs::symlink_metadata(to.join(name)).is_ok())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    if !in_both.is_empty() {
        return kept(format!(
            "both folders hold {}; with one of each left, the rest move",
            in_both.join(", ")
        ));
    }
    let mut moved = Vec::new();
    for name in &names {
        if let Err(err) = rename(&from.join(name), &to.join(name)) {
            for back in moved.iter().rev() {
                let _ = rename(&to.join(back), &from.join(back));
            }
            return kept(format!(
                "{} could not be moved: {err}",
                from.join(name).display()
            ));
        }
        moved.push(name.clone());
    }
    match std::fs::remove_dir(&from) {
        Ok(()) => ConfigMove::Moved {
            from: from.clone(),
            to: to.clone(),
        },
        Err(err) => {
            for back in moved.iter().rev() {
                let _ = rename(&to.join(back), &from.join(back));
            }
            kept(format!("it could not be removed once empty: {err}"))
        }
    }
}

/// The variables that move one folder each.
pub const DATA_DIR_VAR: &str = "FLOTI_DATA_DIR";
pub const LOG_DIR_VAR: &str = "FLOTI_LOG_DIR";
pub const CONFIG_DIR_VAR: &str = "FLOTI_CONFIG_DIR";
pub const RUNTIME_DIR_VAR: &str = "FLOTI_RUNTIME_DIR";

/// Set (to anything) by a client that starts a host whose config folder may
/// be another host's to write, such as a development or check run of Helm
/// beside the installed one: that host reads its config folder and writes
/// nothing in it.
pub const CONFIG_READ_ONLY_VAR: &str = "FLOTI_CONFIG_READ_ONLY";

/// Folders moved away from where the OS's layout puts them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderOverrides {
    pub data: Option<PathBuf>,
    pub logs: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub runtime: Option<PathBuf>,
}

impl FolderOverrides {
    /// The overrides `env` sets. A relative path is refused rather than read
    /// against whatever folder the host happened to start in.
    pub fn from_env(env: &UserEnv) -> Result<Self> {
        let read = |name: &str| -> Result<Option<PathBuf>> {
            match env.var(name).map(PathBuf::from) {
                Some(path) if !path.is_absolute() => Err(Error::Refused(format!(
                    "{name} must be an absolute path, and {} is not.",
                    path.display()
                ))),
                other => Ok(other),
            }
        };
        Ok(Self {
            data: read(DATA_DIR_VAR)?,
            logs: read(LOG_DIR_VAR)?,
            config: read(CONFIG_DIR_VAR)?,
            runtime: read(RUNTIME_DIR_VAR)?,
        })
    }

    /// Every folder in its own subfolder of `root`.
    pub fn under(root: &Path) -> Self {
        Self {
            data: Some(root.join("data")),
            logs: Some(root.join("logs")),
            config: Some(root.join("config")),
            runtime: Some(root.join("run")),
        }
    }

    /// The folders, when every one is overridden and the layout has nothing
    /// left to say.
    fn complete(&self) -> Option<Folders> {
        Some(Folders {
            data: self.data.clone()?,
            logs: self.logs.clone()?,
            config: self.config.clone()?,
            runtime: self.runtime.clone()?,
        })
    }

    /// `folders` with each overridden folder replaced.
    fn apply(self, folders: Folders) -> Folders {
        Folders {
            data: self.data.unwrap_or(folders.data),
            logs: self.logs.unwrap_or(folders.logs),
            config: self.config.unwrap_or(folders.config),
            runtime: self.runtime.unwrap_or(folders.runtime),
        }
    }

    /// The folders `layout` gives for `env`, with these overrides applied.
    /// When every folder is overridden the layout is not consulted, so a
    /// user with no home folder still resolves.
    pub fn resolve(self, layout: &impl FolderLayout, env: &UserEnv) -> Result<Folders> {
        match self.complete() {
            Some(folders) => Ok(folders),
            None => Ok(self.apply(layout.resolve(env)?)),
        }
    }
}

/// The host's folders for the user `env` describes: this OS's layout, with
/// the `FLOTI_*_DIR` overrides `env` sets.
pub fn resolve(env: &UserEnv) -> Result<Folders> {
    FolderOverrides::from_env(env)?.resolve(&Native, env)
}
