//! What macOS and Linux share: a login shell's `PATH`, learned by reading
//! the system's list and the user's startup files as text, never running
//! them.
//!
//! The folders a `PATH` line adds are read from it: every word of a line
//! that sets `PATH` (an assignment or export of `PATH`, zsh's `path` array,
//! fish's `fish_add_path` and `fish_user_paths`, or Homebrew's `brew
//! shellenv`) that is a full path or starts with `$HOME`, `${HOME}` or `~`,
//! its words split as the shell splits them, so a quoted folder keeps its
//! spaces.
//! A line inside a condition counts as if the condition held: Ubuntu's
//! `~/.profile` adds `~/.local/bin` only when that folder exists at sign-in,
//! and it does once floti is in it.

use std::io;
use std::path::{Path, PathBuf};

use crate::platform::folders::UserEnv;

/// Where one OS keeps what a login shell's `PATH` starts with.
pub(super) struct SystemPath {
    /// Folders every login shell has.
    pub standard: &'static [&'static str],
    /// Files, under the root, that list one folder per line (`/etc/paths`).
    pub listed_in: &'static [&'static str],
    /// Folders, under the root, of files that do the same (`/etc/paths.d`).
    pub listed_in_folders: &'static [&'static str],
    /// Files, under the root, with a `PATH=` line (`/etc/environment`).
    pub assigned_in: &'static [&'static str],
}

/// The shell `$SHELL` names, by the startup files it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shell {
    Bash,
    Zsh,
    Fish,
    /// sh, dash, ksh, or a `$SHELL` that is not set.
    Other,
}

impl Shell {
    fn of(env: &UserEnv) -> Self {
        let named = env
            .var("SHELL")
            .map(|shell| shell.to_string_lossy().into_owned())
            .unwrap_or_default();
        // The last word of the path, split on `/` as a POSIX path is.
        match named.rsplit('/').next().unwrap_or_default() {
            "bash" => Self::Bash,
            "zsh" => Self::Zsh,
            "fish" => Self::Fish,
            _ => Self::Other,
        }
    }

    /// The startup files this shell reads under `home`, the one a fix
    /// appends to first.
    fn startup_files(self, home: &Path) -> Vec<PathBuf> {
        let at = |file: &str| {
            file.split('/')
                .fold(home.to_path_buf(), |path, part| path.join(part))
        };
        match self {
            // A login bash reads the first of these that exists, and an
            // interactive one that is not a login `~/.bashrc`.
            Self::Bash => {
                let login = [".bash_profile", ".bash_login", ".profile"]
                    .into_iter()
                    .map(at)
                    .find(|file| file.exists())
                    .unwrap_or_else(|| at(".profile"));
                vec![login, at(".bashrc")]
            }
            Self::Zsh => [".zshenv", ".zprofile", ".zshrc", ".zlogin"]
                .into_iter()
                .map(at)
                .collect(),
            Self::Fish => vec![at(".config/fish/config.fish")],
            Self::Other => vec![at(".profile")],
        }
    }
}

/// Whether `floti` is on the `PATH` a login shell of the user `env`
/// describes gets, with home `home` and the system's files read under
/// `root` (`/` for the real one): its own folder is one of that `PATH`'s, or
/// a `floti` in one of them is this one by another name (a Homebrew link into
/// its Cellar). `Err` is why that could not be told: a file that could not be
/// read.
pub(super) fn has(
    system: &SystemPath,
    root: &Path,
    env: &UserEnv,
    home: &Path,
    floti: &Path,
) -> Result<bool, String> {
    let folders = path_folders(system, root, Shell::of(env), home)?;
    let own = floti.parent().unwrap_or(floti);
    let this = std::fs::canonicalize(floti).ok();
    let name = floti.file_name().unwrap_or_default();
    Ok(folders.iter().any(|folder| {
        same(folder, own)
            || this.as_ref().is_some_and(|this| {
                std::fs::canonicalize(folder.join(name)).ok().as_ref() == Some(this)
            })
    }))
}

/// The folders on a login shell's `PATH`, as far as they can be read.
fn path_folders(
    system: &SystemPath,
    root: &Path,
    shell: Shell,
    home: &Path,
) -> Result<Vec<PathBuf>, String> {
    let mut folders: Vec<PathBuf> = system.standard.iter().map(PathBuf::from).collect();
    let mut listings: Vec<PathBuf> = system
        .listed_in
        .iter()
        .map(|file| under(root, file))
        .collect();
    for folder in system.listed_in_folders {
        listings.extend(listing_files(&under(root, folder))?);
    }
    for file in listings {
        folders.extend(
            read(&file)?
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with('/'))
                .map(PathBuf::from),
        );
    }
    let assigned = system.assigned_in.iter().map(|file| under(root, file));
    for file in assigned.chain(shell.startup_files(home)) {
        for line in read(&file)?.lines() {
            folders.extend(path_entries(line, home));
        }
    }
    Ok(folders)
}

/// The line that puts `folder` on the `PATH` of the shell `$SHELL` names,
/// appended to the first startup file [`has`] reads for that shell.
/// Takes floti's line for `folder` out of the user's startup files, for
/// `floti uninstall`, and answers whether it changed anything. The install
/// script and `floti install-self` add no line to a startup file on macOS or
/// Linux, so there is none of Floti's to take out, and nothing is read or
/// changed: a line naming the folder is the user's own, written from
/// `floti doctor`'s fix or by hand, and stays. Where the install script comes
/// to add a line of its own, its removal goes here, and only that line goes.
pub(super) fn remove(_env: &UserEnv, _folder: &Path) -> Result<bool, String> {
    Ok(false)
}

pub(super) fn fix(env: &UserEnv, folder: &Path) -> String {
    let written = env
        .home
        .as_deref()
        .and_then(|home| folder.strip_prefix(home).ok())
        .map_or_else(
            || folder.display().to_string(),
            |inside| format!("$HOME/{}", slashed(inside)),
        );
    let quoted = double_quoted_inside(&written);
    let shell = Shell::of(env);
    if shell == Shell::Fish {
        let line = format!("fish_add_path -g \"{quoted}\"");
        return format!(
            "mkdir -p ~/.config/fish && echo {} >> ~/.config/fish/config.fish",
            single_quoted(&line)
        );
    }
    let file = env.home.as_deref().map_or_else(
        || "~/.profile".to_owned(),
        |home| {
            let first = shell.startup_files(home).remove(0);
            first.strip_prefix(home).map_or_else(
                |_| first.display().to_string(),
                |inside| format!("~/{}", slashed(inside)),
            )
        },
    );
    let export = format!("export PATH=\"{quoted}:$PATH\"");
    format!("echo {} >> {file}", single_quoted(&export))
}

/// `relative` with `/` between its parts, as a POSIX shell writes it.
fn slashed(relative: &Path) -> String {
    relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// `word` in single quotes, as a POSIX shell reads it back as it is.
fn single_quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// `text` escaped to sit inside double quotes, keeping a leading `$HOME`.
fn double_quoted_inside(text: &str) -> String {
    let (head, rest) = text
        .strip_prefix("$HOME")
        .map_or(("", text), |rest| ("$HOME", rest));
    let mut out = head.to_owned();
    for c in rest.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn under(root: &Path, absolute: &str) -> PathBuf {
    root.join(absolute.trim_start_matches('/'))
}

/// The file's text, or nothing for one that is not there.
fn read(path: &Path) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(err) if is_absent(&err) => Ok(String::new()),
        Err(err) => Err(format!("could not read {}: {err}", path.display())),
    }
}

fn is_absent(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// The files in `folder`, or none for a folder that is not there.
fn listing_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    match std::fs::read_dir(folder) {
        Ok(entries) => Ok(entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect()),
        Err(err) if is_absent(&err) => Ok(Vec::new()),
        Err(err) => Err(format!("could not read {}: {err}", folder.display())),
    }
}

/// The words that set `PATH`, each standing alone: `MANPATH` and
/// `PYTHONPATH` are other variables, and `manpath=(` another array.
const SETS_PATH: [&str; 6] = [
    "PATH",
    "path=(",
    "path+=(",
    "fish_add_path",
    "fish_user_paths",
    "shellenv",
];

/// Whether `line`, not a comment, sets `PATH`.
fn sets_path(line: &str) -> bool {
    let line = line.trim_start();
    if line.starts_with('#') {
        return false;
    }
    let part_of_a_name = |c: char| c.is_ascii_alphanumeric() || c == '_';
    SETS_PATH.iter().any(|word| {
        line.match_indices(word).any(|(at, _)| {
            let before = line[..at].chars().next_back();
            let after = line[at + word.len()..].chars().next();
            !before.is_some_and(part_of_a_name)
                && (word.ends_with('(') || !after.is_some_and(part_of_a_name))
        })
    })
}

/// The folders `line` adds to `PATH`, when it sets it: each word that is a
/// full path or under `$HOME`, `${HOME}` or `~`, and for `brew shellenv`, the
/// `bin` and `sbin` of the Homebrew it names.
fn path_entries(line: &str, home: &Path) -> Vec<PathBuf> {
    if !sets_path(line) {
        return Vec::new();
    }
    let mut entries = Vec::new();
    for word in shell_words(line) {
        let Some(path) = as_path(&word, home) else {
            continue;
        };
        if line.contains("shellenv")
            && path.file_name().is_some_and(|name| name == "brew")
            && let Some(bin) = path.parent()
        {
            entries.push(bin.to_path_buf());
            if let Some(prefix) = bin.parent() {
                entries.push(prefix.join("sbin"));
            }
        }
        entries.push(path);
    }
    entries
}

/// Where a character of a line stands, as a shell reads it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Quoting {
    /// Inside `"..."`: a space is part of the word.
    Double,
    /// Inside `'...'`: everything is part of the word.
    Single,
    /// Inside `$(...)`: words again, as outside any quote.
    Command,
}

/// The words of `line` that could name a folder, split as a shell splits
/// them: on spaces, tabs, `=`, `(`, `)` and `;` outside quotes, and on `:`,
/// `PATH`'s own separator, everywhere. A quoted part keeps its spaces and
/// joins what touches it (`"$HOME/my tools"/bin` is one word), and `$(...)`
/// inside double quotes starts words of its own, as in
/// `eval "$("/opt/my brew/bin/brew" shellenv)"`.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut stack: Vec<Quoting> = Vec::new();
    let mut chars = line.chars().peekable();
    let mut end = |word: &mut String| {
        if !word.is_empty() {
            words.push(std::mem::take(word));
        }
    };
    while let Some(c) = chars.next() {
        let substitution = c == '$' && chars.peek() == Some(&'(');
        match stack.last().copied() {
            Some(Quoting::Single) => match c {
                '\'' => {
                    stack.pop();
                }
                ':' => end(&mut word),
                _ => word.push(c),
            },
            Some(Quoting::Double) => match c {
                '"' => {
                    stack.pop();
                }
                _ if substitution => {
                    chars.next();
                    end(&mut word);
                    stack.push(Quoting::Command);
                }
                ':' => end(&mut word),
                _ => word.push(c),
            },
            outside @ (None | Some(Quoting::Command)) => match c {
                '"' => stack.push(Quoting::Double),
                '\'' => stack.push(Quoting::Single),
                _ if substitution => {
                    chars.next();
                    end(&mut word);
                    stack.push(Quoting::Command);
                }
                ')' => {
                    end(&mut word);
                    if outside == Some(Quoting::Command) {
                        stack.pop();
                    }
                }
                ':' | '=' | '(' | ';' | ' ' | '\t' => end(&mut word),
                _ => word.push(c),
            },
        }
    }
    end(&mut word);
    words
}

/// `word` as a folder: a full path, or one under the home folder.
fn as_path(word: &str, home: &Path) -> Option<PathBuf> {
    let word = word.trim_end_matches('/');
    if word.starts_with('/') {
        return Some(PathBuf::from(word));
    }
    for home_word in ["$HOME", "${HOME}", "~"] {
        if word == home_word {
            return Some(home.to_path_buf());
        }
        if let Some(inside) = word
            .strip_prefix(home_word)
            .and_then(|rest| rest.strip_prefix('/'))
        {
            return Some(
                inside
                    .split('/')
                    .fold(home.to_path_buf(), |path, part| path.join(part)),
            );
        }
    }
    None
}

/// Whether `a` and `b` are one folder, a trailing separator aside.
fn same(a: &Path, b: &Path) -> bool {
    a.components().eq(b.components())
}
