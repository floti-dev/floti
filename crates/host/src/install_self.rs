//! `floti install-self`: puts the floti that runs it where the install
//! scripts put floti, the user's own folder ([`Standalone::folder`]:
//! `%LOCALAPPDATA%\Programs\Floti\bin` or `~/.local/bin`), with the Node
//! runtime shipped beside it, then runs `floti setup` from there. It is what
//! the desktop's Windows installer runs from the floti it carries, and what
//! the desktop runs at start on any OS when the floti there is missing or
//! older than its own, so a computer has one floti, in one place, however
//! Floti came to it.
//!
//! 1. floti: copied beside the one there under a name of its own, then put
//!    in its place ([`Standalone::replace`]): a copy there that is running,
//!    such as the host, keeps running, and is taken over by Restart host to
//!    update, never ended here. A copy there that is this one byte for byte
//!    is left as it is. Copies an earlier install put aside are removed once
//!    nothing runs them.
//! 2. The Node runtime: the `node` folder beside this floti, when there is
//!    one, file by file the same way, into `node` beside the installed one,
//!    where the host looks for it. A `node` there that is not a folder (a
//!    file or a link of the user's own) is left as it is, and said so.
//!    Links in the source are not followed.
//! 3. `PATH`: when floti's folder is not on the user's `PATH`, it is put
//!    there ([`UserPath::add`]): on Windows in the user's `Path` value, kept
//!    expandable, so a terminal or an ssh login started after it finds
//!    `floti`. On macOS and Linux nothing is changed and `floti doctor`
//!    gives the line, as after the install script.
//! 4. `floti setup`, run from the installed floti, so the host registered
//!    to start at sign-in is that one; its lines are its own.
//!
//! It prints where floti is, and each change it made, one line each; a
//! failure is one line on standard error starting `floti install-self: `.
//! A failure to place floti or its Node stops it with status 1 before
//! setup; otherwise the status is setup's, or 1 when `PATH` could not be
//! changed.

use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform::folders::UserEnv;
use crate::platform::node_runtime::SHIPPED_FOLDER;
use crate::platform::standalone::{Native as NativeStandalone, Standalone};
use crate::platform::user_path::{Native as NativeUserPath, UserPath};

/// What installing asks of the computer beyond copying files.
pub trait InstallSystem {
    /// Whether `floti` is on the user's `PATH`, or why that could not be told.
    fn on_path(&self, floti: &Path) -> Result<bool, String>;

    /// Puts `folder` on the user's `PATH`; whether anything was changed.
    fn add_to_path(&self, folder: &Path) -> Result<bool, String>;

    /// Runs `floti setup` from `floti`, and answers its exit status.
    fn setup(&self, floti: &Path) -> Result<u8, String>;
}

/// This computer, for the user this process runs as.
pub struct NativeSystem {
    env: UserEnv,
}

impl InstallSystem for NativeSystem {
    fn on_path(&self, floti: &Path) -> Result<bool, String> {
        NativeUserPath.has(&self.env, floti)
    }

    fn add_to_path(&self, folder: &Path) -> Result<bool, String> {
        NativeUserPath.add(&self.env, folder)
    }

    fn setup(&self, floti: &Path) -> Result<u8, String> {
        let status = Command::new(floti)
            .arg("setup")
            .status()
            .map_err(|err| format!("could not run {} setup: {err}", floti.display()))?;
        Ok(status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(1))
    }
}

/// Runs `floti install-self` for the floti this process is.
pub fn run() -> u8 {
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    let source = match std::env::current_exe() {
        Ok(source) => source,
        Err(why) => {
            let _ = writeln!(
                err,
                "floti install-self: could not tell where this floti is: {why}"
            );
            return 1;
        }
    };
    let env = UserEnv::from_process();
    let Some(folder) = NativeStandalone.folder(&env) else {
        let _ = writeln!(
            err,
            "floti install-self: Floti cannot find your home folder, so it does not know where to put floti."
        );
        return 1;
    };
    install_self(
        &source,
        &folder,
        &NativeStandalone,
        &NativeSystem { env },
        &mut out,
        &mut err,
    )
}

/// Installs `source`, and the Node runtime beside it, into `folder` through
/// `place`, puts `folder` on the user's `PATH` and runs setup from the
/// installed floti through `system`, writing its lines to `out` and a
/// failure to `err`. Answers the exit status.
pub fn install_self(
    source: &Path,
    folder: &Path,
    place: &dyn Standalone,
    system: &dyn InstallSystem,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut say = |line: String| {
        let _ = writeln!(out, "{line}").and_then(|()| out.flush());
    };
    let placed = place_all(source, folder, place);
    let floti = match placed {
        Ok((floti, notes)) => {
            say(format!("floti is at {}.", floti.display()));
            for note in notes {
                say(note);
            }
            floti
        }
        Err(why) => {
            let _ = writeln!(err, "floti install-self: {why}");
            return 1;
        }
    };
    let path_failed = match system.on_path(&floti).and_then(|on| {
        if on {
            Ok(false)
        } else {
            system.add_to_path(folder)
        }
    }) {
        Ok(true) => {
            say(format!(
                "Put {} on your PATH, for terminals and logins started from now on.",
                folder.display()
            ));
            false
        }
        Ok(false) => false,
        Err(why) => {
            let _ = writeln!(
                err,
                "floti install-self: could not put {} on your PATH: {why}",
                folder.display()
            );
            true
        }
    };
    let code = match system.setup(&floti) {
        Ok(code) => code,
        Err(why) => {
            let _ = writeln!(err, "floti install-self: {why}");
            1
        }
    };
    if path_failed && code == 0 { 1 } else { code }
}

/// Puts floti and its Node in `folder`: the installed floti's path, and a
/// line for anything left as it was.
fn place_all(
    source: &Path,
    folder: &Path,
    place: &dyn Standalone,
) -> Result<(PathBuf, Vec<String>), String> {
    let name = source
        .file_name()
        .ok_or_else(|| format!("{} names no file", source.display()))?;
    std::fs::create_dir_all(folder)
        .map_err(|why| format!("could not make {}: {why}", folder.display()))?;
    let floti = folder.join(name);
    place_file(source, &floti, place)?;
    let mut notes = Vec::new();
    if let Some(node) = source.parent().map(|parent| parent.join(SHIPPED_FOLDER))
        && is_real_folder(&node)
    {
        let target = folder.join(SHIPPED_FOLDER);
        if let Some(note) = place_folder(&node, &target, place)? {
            notes.push(note);
        }
    }
    Ok((floti, notes))
}

/// Whether `path` is a folder, and not a link to one.
fn is_real_folder(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
}

/// Puts each file under `source` at the same place under `target`, making
/// its folders. A `target` that is there but not a folder is left as it is,
/// and the answer says so; a link in `source` is not followed.
fn place_folder(
    source: &Path,
    target: &Path,
    place: &dyn Standalone,
) -> Result<Option<String>, String> {
    match std::fs::symlink_metadata(target) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        Ok(_) => {
            return Ok(Some(format!(
                "Left {} as it is: it is not a folder Floti made, so a host started from beside it has no Node for plugin services.",
                target.display()
            )));
        }
        Err(why) if why.kind() == ErrorKind::NotFound => std::fs::create_dir_all(target)
            .map_err(|why| format!("could not make {}: {why}", target.display()))?,
        Err(why) => return Err(format!("could not look at {}: {why}", target.display())),
    }
    let entries = std::fs::read_dir(source)
        .map_err(|why| format!("could not read {}: {why}", source.display()))?;
    for entry in entries {
        let entry = entry.map_err(|why| format!("could not read {}: {why}", source.display()))?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        let kind = std::fs::symlink_metadata(&from)
            .map_err(|why| format!("could not look at {}: {why}", from.display()))?
            .file_type();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if let Some(note) = place_folder(&from, &to, place)? {
                return Ok(Some(note));
            }
        } else {
            place_file(&from, &to, place)?;
        }
    }
    Ok(None)
}

/// Puts `source` at `target`, unless `target` is `source` itself or the
/// same bytes: copied beside `target` under a name of its own, then put in
/// its place. Copies an earlier install put aside, and its own copies left
/// by one that stopped part way, are removed first where nothing runs them.
fn place_file(source: &Path, target: &Path, place: &dyn Standalone) -> Result<(), String> {
    if let (Ok(from), Ok(to)) = (std::fs::canonicalize(source), std::fs::canonicalize(target))
        && from == to
    {
        return Ok(());
    }
    place.remove_asides(target);
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} names no file", target.display()))?;
    remove_copies(target, &name);
    if target.is_file() && same_bytes(source, target).unwrap_or(false) {
        return Ok(());
    }
    let copy = target.with_file_name(format!(".{name}{COPY}{}", std::process::id()));
    if let Err(why) = std::fs::copy(source, &copy) {
        let _ = std::fs::remove_file(&copy);
        return Err(format!(
            "could not copy {} to {}: {why}",
            source.display(),
            copy.display()
        ));
    }
    place.replace(target, &copy).map_err(|why| {
        let _ = std::fs::remove_file(&copy);
        format!("could not put {} in place: {why}", target.display())
    })
}

/// What the name of a copy waiting to be put in place has after the
/// target's name: `.floti.exe.floti-install-4120`.
const COPY: &str = ".floti-install-";

/// Removes the copies of `target` (named `name`) that an install which
/// stopped part way left beside it.
fn remove_copies(target: &Path, name: &str) {
    let Some(folder) = target.parent() else {
        return;
    };
    let prefix = format!(".{name}{COPY}");
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.filter_map(Result::ok) {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Whether the files at `a` and `b` hold the same bytes.
fn same_bytes(a: &Path, b: &Path) -> io::Result<bool> {
    let (mut a, mut b) = (File::open(a)?, File::open(b)?);
    if a.metadata()?.len() != b.metadata()?.len() {
        return Ok(false);
    }
    let mut left = vec![0u8; 64 * 1024];
    let mut right = vec![0u8; 64 * 1024];
    loop {
        let read = a.read(&mut left)?;
        if read == 0 {
            return Ok(true);
        }
        b.read_exact(&mut right[..read])?;
        if left[..read] != right[..read] {
            return Ok(false);
        }
    }
}
