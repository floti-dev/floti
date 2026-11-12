//! What macOS and Linux share: a login shell's `PATH`, learned by reading
//! the system's list and the user's startup files as text, never running
//! them.

use std::io;
use std::path::{Path, PathBuf};

use crate::platform::folders::UserEnv;

/// The user's startup files a shell reads, under the home folder: sh's and
/// bash's, zsh's and fish's.
const STARTUP_FILES: [&str; 8] = [
    ".profile",
    ".bash_profile",
    ".bash_login",
    ".bashrc",
    ".zshenv",
    ".zprofile",
    ".zshrc",
    ".config/fish/config.fish",
];

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

/// Whether `folder` is on the `PATH` a login shell of the user whose home
/// is `home` gets, with the system's files read under `root` (`/` for the
/// real one), or why that could not be told: a file there that could not be
/// read.
pub(super) fn has(
    system: &SystemPath,
    root: &Path,
    home: &Path,
    folder: &Path,
) -> Result<bool, String> {
    if system
        .standard
        .iter()
        .any(|standard| same(Path::new(standard), folder))
    {
        return Ok(true);
    }
    for file in system.listed_in {
        if listed(&read(&under(root, file))?, folder) {
            return Ok(true);
        }
    }
    for listing in system.listed_in_folders {
        for file in listing_files(&under(root, listing))? {
            if listed(&read(&file)?, folder) {
                return Ok(true);
            }
        }
    }
    for file in system.assigned_in {
        if read(&under(root, file))?
            .lines()
            .filter(|line| sets_path(line))
            .any(|line| names(line, home, folder))
        {
            return Ok(true);
        }
    }
    for file in STARTUP_FILES {
        let path = file
            .split('/')
            .fold(home.to_path_buf(), |path, part| path.join(part));
        if read(&path)?
            .lines()
            .filter(|line| sets_path(line))
            .any(|line| names(line, home, folder))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The line that puts `folder` on the `PATH` of the shell `$SHELL` names.
pub(super) fn fix(env: &UserEnv, folder: &Path) -> String {
    // The last word of `$SHELL`'s path, split on `/` as a POSIX path is.
    let shell = env
        .var("SHELL")
        .map(|shell| {
            let shell = shell.to_string_lossy();
            shell.rsplit('/').next().unwrap_or_default().to_owned()
        })
        .unwrap_or_default();
    let written = env
        .home
        .as_deref()
        .and_then(|home| folder.strip_prefix(home).ok())
        .map_or_else(
            || folder.display().to_string(),
            |inside| format!("$HOME/{}", inside.display()),
        );
    if shell == "fish" {
        return format!(
            "fish_add_path {}",
            single_quoted(&folder.display().to_string())
        );
    }
    let file = if shell == "zsh" {
        "~/.zshenv"
    } else {
        "~/.profile"
    };
    let export = format!("export PATH=\"{}:$PATH\"", double_quoted_inside(&written));
    format!("echo {} >> {file}", single_quoted(&export))
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
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(String::new())
        }
        Err(err) => Err(format!("could not read {}: {err}", path.display())),
    }
}

/// The files in `folder`, or none for a folder that is not there.
fn listing_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    match std::fs::read_dir(folder) {
        Ok(entries) => Ok(entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect()),
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(Vec::new())
        }
        Err(err) => Err(format!("could not read {}: {err}", folder.display())),
    }
}

/// Whether `text`, one folder per line, lists `folder`.
fn listed(text: &str, folder: &Path) -> bool {
    text.lines()
        .map(str::trim)
        .any(|line| !line.is_empty() && same(Path::new(line), folder))
}

/// Whether `line`, not a comment, sets `PATH`: an assignment or export of
/// it, zsh's `path` array, or fish's `fish_add_path` and `fish_user_paths`.
fn sets_path(line: &str) -> bool {
    let line = line.trim_start();
    !line.starts_with('#')
        && [
            "PATH",
            "path=(",
            "path+=(",
            "fish_add_path",
            "fish_user_paths",
        ]
        .iter()
        .any(|word| line.contains(word))
}

/// Whether `line` names `folder`, as a full path or under `$HOME`,
/// `${HOME}` or `~`, as a whole entry: followed by the end, a separator, a
/// quote or a blank.
fn names(line: &str, home: &Path, folder: &Path) -> bool {
    let folder = folder.display().to_string();
    let folder = folder.trim_end_matches('/');
    let mut spellings = vec![folder.to_owned()];
    if let Ok(inside) = Path::new(folder).strip_prefix(home) {
        let inside = inside.display().to_string();
        for home_word in ["$HOME", "${HOME}", "~"] {
            spellings.push(format!("{home_word}/{inside}"));
        }
    }
    spellings.iter().any(|spelling| {
        line.match_indices(spelling.as_str()).any(|(at, _)| {
            let starts = line[..at]
                .chars()
                .next_back()
                .is_none_or(|c| matches!(c, ':' | '"' | '\'' | '=' | '(' | ' ' | '\t'));
            let ends = line[at + spelling.len()..]
                .chars()
                .next()
                .is_none_or(|c| matches!(c, ':' | '"' | '\'' | ' ' | '\t' | '/' | ';' | ')'));
            starts && ends
        })
    })
}

/// Whether `a` and `b` are one folder, a trailing `/` aside.
fn same(a: &Path, b: &Path) -> bool {
    let a = a.display().to_string();
    let b = b.display().to_string();
    a.trim_end_matches('/') == b.trim_end_matches('/')
}
