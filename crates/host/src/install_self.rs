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
//!    is left as it is, and so is one that says a newer version than this
//!    one (`floti update` put it there), with its Node, unless `--force`
//!    asks for this one. Copies an earlier install or update put aside are
//!    removed once nothing runs them. On a computer whose own CPU is arm64
//!    ([`Standalone::native_cpu`]), the arm64 build beside this floti
//!    ([`ARM64_BUILD`], which the Windows desktop installer carries beside
//!    its x64 floti) is what is put in place, by the same rules, even when
//!    this floti is x64 emulated there; so a native arm64 floti install.ps1
//!    put there, of the same version, is left as it is.
//! 2. The Node runtime: the `node` folder beside this floti, when there is
//!    one, file by file the same way, into `node` beside the installed one,
//!    where the host looks for it. A `node`, or a folder in it, that is not
//!    a folder there (a file or a link of the user's own) is left as it is,
//!    and said so, and the rest is put in place. Links in the source are
//!    not followed.
//! 3. `PATH`: when floti's folder is not on the user's `PATH`, it is put
//!    there ([`UserPath::add`]): on Windows in the user's `Path` value, kept
//!    expandable, so a terminal or an ssh login started after it finds
//!    `floti`. On macOS and Linux nothing is changed and `floti doctor`
//!    gives the line, as after the install script.
//! 4. `floti setup --no-desktop`, run from the installed floti, so the host
//!    registered to start at sign-in is that one; its lines are its own.
//!    The desktop app that runs install-self is there already, so setup
//!    neither asks about it nor installs one.
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
use crate::update::versions::Version;
use crate::version::FLOTI_VERSION;

/// What install-self runs setup with: never a question about, or an install
/// of, the desktop app, which is there already.
pub const SETUP_ARGS: [&str; 2] = ["setup", "--no-desktop"];

/// What installing asks of the computer beyond copying files.
pub trait InstallSystem {
    /// Whether `floti` is on the user's `PATH`, or why that could not be told.
    fn on_path(&self, floti: &Path) -> Result<bool, String>;

    /// Puts `folder` on the user's `PATH`; whether anything was changed.
    fn add_to_path(&self, folder: &Path) -> Result<bool, String>;

    /// Runs `floti setup --no-desktop` from `floti`, and answers its exit
    /// status.
    fn setup(&self, floti: &Path) -> Result<u8, String>;

    /// The version the floti at `floti` says (`floti version`), or `None`
    /// when it says none.
    fn version_of(&self, floti: &Path) -> Option<String>;

    /// This computer's own CPU ([`Standalone::native_cpu`]), whatever this
    /// floti was built for.
    fn native_cpu(&self) -> &'static str;
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
            .args(SETUP_ARGS)
            .status()
            .map_err(|err| format!("could not run {} setup: {err}", floti.display()))?;
        Ok(status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(1))
    }

    fn version_of(&self, floti: &Path) -> Option<String> {
        let output = Command::new(floti).arg("version").output().ok()?;
        version_in(&String::from_utf8_lossy(&output.stdout))
    }

    fn native_cpu(&self) -> &'static str {
        NativeStandalone.native_cpu()
    }
}

/// The version a `floti version` line names: `floti 3.0.0 protocol 3.0
/// windows x64` is `3.0.0`.
pub fn version_in(printed: &str) -> Option<String> {
    printed.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next() == Some("floti"))
            .then(|| words.next())
            .flatten()
            .map(str::to_owned)
    })
}

/// What `floti install-self` is asked: nothing, or `--force` to put this
/// floti in place over a newer one.
pub fn parse(args: &[String]) -> Option<bool> {
    match args {
        [] => Some(false),
        [force] if force == "--force" => Some(true),
        _ => None,
    }
}

/// Runs `floti install-self` for the floti this process is, over a newer
/// floti there only when `force`.
pub fn run(force: bool) -> u8 {
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
        &Install {
            source: &source,
            version: FLOTI_VERSION,
            folder: &folder,
            force,
        },
        &NativeStandalone,
        &NativeSystem { env },
        &mut out,
        &mut err,
    )
}

/// What to install where.
pub struct Install<'a> {
    /// The floti to put in place, with its Node beside it.
    pub source: &'a Path,
    /// Its version.
    pub version: &'a str,
    /// The folder to put it in.
    pub folder: &'a Path,
    /// Whether to put it over a floti there that says a newer version.
    pub force: bool,
}

/// Installs `install.source`, and the Node runtime beside it, into
/// `install.folder` through `place`, puts that folder on the user's `PATH`
/// and runs setup from the installed floti through `system`, writing its
/// lines to `out` and a failure to `err`. Answers the exit status.
pub fn install_self(
    install: &Install<'_>,
    place: &dyn Standalone,
    system: &dyn InstallSystem,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut say = |line: String| {
        let _ = writeln!(out, "{line}").and_then(|()| out.flush());
    };
    let folder = install.folder;
    let placed = place_all(install, place, system);
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

/// Puts floti and its Node in the folder: the installed floti's path, and a
/// line for anything left as it was.
fn place_all(
    install: &Install<'_>,
    place: &dyn Standalone,
    system: &dyn InstallSystem,
) -> Result<(PathBuf, Vec<String>), String> {
    let Install {
        source,
        folder,
        version,
        force,
    } = *install;
    let name = source
        .file_name()
        .ok_or_else(|| format!("{} names no file", source.display()))?;
    std::fs::create_dir_all(folder)
        .map_err(|why| format!("could not make {}: {why}", folder.display()))?;
    let floti = folder.join(name);
    let chosen = chosen_source(source, system.native_cpu());
    if !force && let Some(newer) = newer_there(&floti, &chosen, version, system) {
        return Ok((
            floti.clone(),
            vec![format!(
                "Left {} as it is: it is floti {newer}, newer than this floti {version}; floti install-self --force puts this one there.",
                floti.display()
            )],
        ));
    }
    place_file(&chosen, &floti, place)?;
    let mut notes = Vec::new();
    if let Some(node) = source.parent().map(|parent| parent.join(SHIPPED_FOLDER))
        && is_real_folder(&node)
    {
        place_folder(&node, &folder.join(SHIPPED_FOLDER), place, &mut notes)?;
    }
    Ok((floti, notes))
}

/// The arm64 build of floti a Windows desktop install carries beside its
/// x64 one (`resources\floti-arm64.exe`, the release's
/// `floti-windows-arm64.exe`).
pub const ARM64_BUILD: &str = "floti-arm64.exe";

/// The floti to put in place for `source`, the floti running install-self,
/// on a computer whose own CPU is `native_cpu`: the arm64 build beside it
/// ([`ARM64_BUILD`]) when the computer is arm64 and that file is there, so an
/// Arm computer gets a native floti even from an x64 one emulated there;
/// otherwise `source` itself.
pub fn chosen_source(source: &Path, native_cpu: &str) -> PathBuf {
    let arm64 = source.with_file_name(ARM64_BUILD);
    if native_cpu == "arm64" && arm64.is_file() {
        arm64
    } else {
        source.to_path_buf()
    }
}

/// The version of the floti at `target` when it is another file than
/// `source` and says a version newer than `version`.
fn newer_there(
    target: &Path,
    source: &Path,
    version: &str,
    system: &dyn InstallSystem,
) -> Option<String> {
    if !target.is_file() || same_file(source, target) {
        return None;
    }
    let ours = Version::parse(version)?;
    let there = system.version_of(target)?;
    (Version::parse(&there)? > ours).then_some(there)
}

/// Whether `a` and `b` are one file.
fn same_file(a: &Path, b: &Path) -> bool {
    matches!(
        (std::fs::canonicalize(a), std::fs::canonicalize(b)),
        (Ok(a), Ok(b)) if a == b
    )
}

/// Whether `path` is a folder, and not a link to one.
fn is_real_folder(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
}

/// Puts each file under `source` at the same place under `target`, making
/// its folders. A `target`, or a folder under it, that is there but not a
/// folder is left as it is, with a line in `notes`, and the rest is still
/// put in place; a link in `source` is not followed.
fn place_folder(
    source: &Path,
    target: &Path,
    place: &dyn Standalone,
    notes: &mut Vec<String>,
) -> Result<(), String> {
    match std::fs::symlink_metadata(target) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        Ok(_) => {
            notes.push(format!(
                "Left {} as it is: it is not a folder Floti made, so a host started from beside it may have no Node for plugin services.",
                target.display()
            ));
            return Ok(());
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
            place_folder(&from, &to, place, notes)?;
        } else {
            place_file(&from, &to, place)?;
        }
    }
    Ok(())
}

/// Puts `source` at `target`, unless `target` is `source` itself or the
/// same bytes: copied beside `target` under a name of its own, then put in
/// its place. Copies an earlier install put aside, and its own copies left
/// by one that stopped part way, are removed first where nothing runs them.
fn place_file(source: &Path, target: &Path, place: &dyn Standalone) -> Result<(), String> {
    if same_file(source, target) {
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
pub(crate) fn remove_copies(target: &Path, name: &str) {
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
