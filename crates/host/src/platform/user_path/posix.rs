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
//!
//! floti's own line ([`floti_line`]) is the one `floti doctor` gives, the
//! install appends ([`add`]) and `floti uninstall` takes out ([`remove`]):
//! it ends in [`MARKER`], so a line the user wrote is never taken for it.

use std::fs::OpenOptions;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::{Added, TakesEffect};
use crate::platform::folders::UserEnv;

/// What ends floti's own `PATH` line in a startup file, a shell comment, so
/// that `floti uninstall` takes out that line and no line the user wrote.
pub const MARKER: &str = "# added by floti";

/// Every startup file under the home folder that a shell floti knows reads,
/// which `floti uninstall` looks in for floti's line, whatever the user's
/// shell is by then.
const EVERY_STARTUP_FILE: [&str; 9] = [
    ".bash_profile",
    ".bash_login",
    ".profile",
    ".bashrc",
    ".zshenv",
    ".zprofile",
    ".zshrc",
    ".zlogin",
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
    /// Whether a new terminal starts a login shell, which reads the login
    /// files (macOS's Terminal does), or inherits the `PATH` its desktop
    /// session started with (a Linux desktop's terminals).
    pub terminals_are_logins: bool,
}

/// The shell `$SHELL` names, by the startup files it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shell {
    Bash,
    Zsh,
    Fish,
    /// sh, and the shells that read what it reads at a login: dash, ash,
    /// ksh and mksh.
    Sh,
    /// A `$SHELL` that is not set, or one naming a shell floti knows no
    /// startup file of (tcsh, nushell and the like).
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
            "sh" | "dash" | "ash" | "ksh" | "mksh" => Self::Sh,
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
            Self::Sh | Self::Other => vec![at(".profile")],
        }
    }

    /// Whether every shell of this kind reads the file floti's line goes in,
    /// a login or not: zsh's `~/.zshenv` and fish's `config.fish`, but not
    /// bash's or sh's login file.
    fn every_shell_reads_the_first_file(self) -> bool {
        matches!(self, Self::Zsh | Self::Fish)
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

/// The command that puts `folder` on the `PATH` of the shell `$SHELL` names:
/// it appends [`floti_line`] to the first startup file [`has`] reads for
/// that shell, the file [`add`] appends it to.
pub(super) fn fix(env: &UserEnv, folder: &Path) -> String {
    let shell = Shell::of(env);
    let line = floti_line(shell, env, folder);
    if shell == Shell::Fish {
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
    format!("echo {} >> {file}", single_quoted(&line))
}

/// Puts `folder` on the `PATH` of the shell `$SHELL` names, on an OS whose
/// login shells start as `system` says: appends [`floti_line`] to the first
/// startup file [`has`] reads for that shell, the one [`fix`] names, and
/// answers that file and what it reaches. The line goes at the end, after a
/// line break when the file does not end in one, and nothing else in the file
/// changes. A link is followed and its target appended to, which keeps its
/// permissions. The file is made only where it is not there, with its folder
/// for fish. A read-only file, or a folder it cannot be made in, is refused
/// and left as it was. A file that has the line already is left as it is
/// ([`Added::Nothing`]), and a shell floti knows no startup file of changes
/// nothing ([`Added::ByHand`]).
pub(super) fn add(system: &SystemPath, env: &UserEnv, folder: &Path) -> Result<Added, String> {
    let shell = Shell::of(env);
    if shell == Shell::Other {
        return Ok(Added::ByHand {
            shell: env
                .var("SHELL")
                .map(|shell| shell.to_string_lossy().into_owned())
                .filter(|shell| !shell.is_empty()),
        });
    }
    let home = env.require_home().map_err(|err| err.to_string())?;
    let file = shell.startup_files(home).remove(0);
    let line = floti_line(shell, env, folder);
    let refused =
        |why: &dyn std::fmt::Display| format!("could not add to {}: {why}", file.display());
    let bytes = read_bytes(&file)?;
    if lines(&bytes).any(|written| written.trim_ascii_end() == line.as_bytes()) {
        return Ok(Added::Nothing);
    }
    if file.exists() {
        refuse_read_only(&file).map_err(|why| refused(&why))?;
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("could not make {}: {err}", parent.display()))?;
    }
    let lead = if bytes.is_empty() || bytes.ends_with(b"\n") {
        ""
    } else {
        "\n"
    };
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(&file)
        .and_then(|mut out| out.write_all(format!("{lead}{line}\n").as_bytes()))
        .map_err(|err| refused(&err))?;
    let takes = if system.terminals_are_logins || shell.every_shell_reads_the_first_file() {
        TakesEffect::Now
    } else {
        TakesEffect::AtNextSignIn
    };
    Ok(Added::Put {
        file: Some(file),
        takes,
    })
}

/// Takes floti's line for `folder` ([`floti_line`], as any shell's is
/// written) out of every startup file of a shell floti knows
/// ([`EVERY_STARTUP_FILE`]), for `floti uninstall`, and answers whether there
/// was one. Only floti's line, marker and all, goes; every other line stays
/// as written, byte for byte and in its order, whatever its encoding. The
/// file is changed in place ([`rewrite`]), through a link where it is one,
/// and a read-only one is refused and left as it was.
pub(super) fn remove(env: &UserEnv, folder: &Path) -> Result<bool, String> {
    let home = env.require_home().map_err(|err| err.to_string())?;
    let flotis = [Shell::Sh, Shell::Fish].map(|shell| floti_line(shell, env, folder));
    let is_flotis = |written: &[u8]| {
        flotis
            .iter()
            .any(|line| written.trim_ascii_end() == line.as_bytes())
    };
    let mut removed = false;
    for file in EVERY_STARTUP_FILE {
        let file = file
            .split('/')
            .fold(home.to_path_buf(), |path, part| path.join(part));
        let bytes = read_bytes(&file)?;
        if !lines(&bytes).any(is_flotis) {
            continue;
        }
        let kept: Vec<u8> = lines(&bytes)
            .filter(|&written| !is_flotis(written))
            .flatten()
            .copied()
            .collect();
        rewrite(&file, &bytes, &kept)?;
        removed = true;
    }
    Ok(removed)
}

/// The lines of `bytes`, each with its line break.
fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes.split_inclusive(|&byte| byte == b'\n')
}

/// Why `file`, or the file it links to, must not be written: it is
/// read-only. A refusal here leaves it as it was even where its owner, or
/// root, could write it anyway.
fn refuse_read_only(file: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(file).map_err(|err| err.to_string())?;
    if metadata.permissions().readonly() {
        Err("it is read-only".to_owned())
    } else {
        Ok(())
    }
}

/// floti's line that puts `folder` on the `PATH` of `shell`, ending in
/// [`MARKER`]: `export PATH="$HOME/.local/bin:$PATH"`, or fish's
/// `fish_add_path -g`, a folder in the home folder written from `$HOME`.
fn floti_line(shell: Shell, env: &UserEnv, folder: &Path) -> String {
    let written = env
        .home
        .as_deref()
        .and_then(|home| folder.strip_prefix(home).ok())
        .map_or_else(
            || folder.display().to_string(),
            |inside| format!("$HOME/{}", slashed(inside)),
        );
    let quoted = double_quoted_inside(&written);
    if shell == Shell::Fish {
        format!("fish_add_path -g \"{quoted}\" {MARKER}")
    } else {
        format!("export PATH=\"{quoted}:$PATH\" {MARKER}")
    }
}

/// Puts `kept`, which is `was` with lines taken out, in place of what
/// `file`, or the file it links to, holds. The same file is written over and
/// cut to its new length, never replaced by another, so it keeps its owner,
/// permissions, hard links, extended attributes and ACLs, and needs no
/// writable folder: a startup file is the user's own, and a dotfile manager
/// or a second link to it would lose track of a file renamed over it. A
/// read-only file is refused before anything is written. The cost is that it
/// is not atomic: a write that fails part way is undone by writing `was`
/// back, and only a crash between the write and the cut, a few hundred
/// bytes, leaves the tail of `was` after `kept`.
fn rewrite(file: &Path, was: &[u8], kept: &[u8]) -> Result<(), String> {
    let refused =
        |why: &dyn std::fmt::Display| format!("could not change {}: {why}", file.display());
    refuse_read_only(file).map_err(|why| refused(&why))?;
    let mut out = OpenOptions::new()
        .write(true)
        .open(file)
        .map_err(|err| refused(&err))?;
    let put = |out: &mut std::fs::File, bytes: &[u8]| {
        out.seek(SeekFrom::Start(0))?;
        out.write_all(bytes)?;
        out.set_len(bytes.len() as u64)?;
        out.flush()
    };
    if let Err(err) = put(&mut out, kept) {
        let _ = put(&mut out, was);
        return Err(refused(&err));
    }
    Ok(())
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

/// The file's text, or nothing for one that is not there, for reading
/// `PATH` lines from; a byte that is not UTF-8 is read as a replacement
/// character, so the text is never written back.
fn read(path: &Path) -> Result<String, String> {
    read_bytes(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// The file's bytes, or none for one that is not there.
fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(err) if is_absent(&err) => Ok(Vec::new()),
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
