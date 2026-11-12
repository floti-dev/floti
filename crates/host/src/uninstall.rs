//! `floti uninstall [--purge]`: takes off this computer what installing
//! Floti put there, either way in (the install scripts, or the desktop
//! installer through `floti install-self`). It is the explicit act, so it
//! asks nothing.
//!
//! In this order, so a run that stops part way leaves nothing half-working,
//! and a run after it carries on where it stopped:
//!
//! 1. The desktop app, while it runs, is closed: one that started the host
//!    would start another once the host stops.
//! 2. The host is stopped for good (`HostStopReason::Stop`), which ends every
//!    session and plain terminal and its session keeper; with no host
//!    answering, a keeper a host left running is ended with what it holds.
//!    The sessions' history stays in the data folder.
//! 3. The start-at-sign-in entry ([`register::unregister`]): the logon task
//!    "Floti host", the LaunchAgent `dev.floti.host` or the user unit
//!    `floti.service`, and in a WSL distro without systemd the record that
//!    makes `floti connect` start the host. Lingering on Linux is a per-user
//!    setting other services may rely on, so it is left on, and said so.
//! 4. The desktop app ([`Installs::removal`]): on Windows by its own NSIS
//!    uninstaller, run silently and waited for; the `.app` on macOS; the
//!    AppImage, its menu entry and icon on Linux.
//! 5. The `PATH` entry ([`UserPath::remove`]): on Windows the folder's entry
//!    in the user's `Path`. On macOS and Linux floti adds none.
//! 6. floti and its Node, from where the install scripts put floti
//!    ([`Standalone::folder`]), with the copies an install or update put
//!    aside or left part way. A running `floti.exe` cannot delete itself on
//!    Windows, so it is renamed aside and deleted once this process has
//!    exited ([`Standalone::delete_after_exit`]).
//!
//! Its data folder, logs and config folder stay unless `--purge`, which then
//! removes them, unlinking every link in them and never walking into one, and
//! Floti's own entry in the OS secret store. Nothing ever touches `~/.claude`,
//! another agent's config, `~/.ssh` or a repository: a folder that is the home
//! folder or holds it, or is or holds or is inside `~/.claude` or `~/.ssh`, is
//! refused rather than removed.
//!
//! A floti that is not the one where the install scripts put floti (one a
//! package manager installed) removes only what it installed: steps 1 to 4
//! and the data, and says in one line that it leaves itself and the `PATH`
//! to whatever installed it.
//!
//! Each step prints one line on standard output, `<what>: <how it went>`,
//! such as `sign-in entry: removed (logon task "Floti host")` or
//! `desktop app: not found`, so a second run says `not found` throughout and
//! exits 0. A step that fails stops the run with one line on standard error,
//! `floti uninstall: <what>: <why>`, and status 1; so does running it from
//! one of Floti's own sessions, which stopping the host would end, before
//! anything is changed.

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use floti_protocol::Knowable;

use crate::connect::START_ON_CONNECT_FILE;
use crate::install_self::remove_copies;
use crate::platform::folders::{self, DATA_DIR_VAR, Folders, UserEnv};
use crate::platform::installs::{self, Desktop, Installs};
use crate::platform::launcher::{self, ProgramLauncher};
use crate::platform::links::{self, remove_tree};
use crate::platform::node_runtime::SHIPPED_FOLDER;
use crate::platform::process_table::{self, ProcessRow, ProcessTable};
use crate::platform::process_tree::{self, Ending, ProcessTree};
use crate::platform::secret_seal::{self, StoreEntry};
use crate::platform::standalone::{self, Standalone};
use crate::platform::startup::register::{self, SignInSystem};
use crate::platform::startup::{self, StartAtSignIn, StartupDefinition, StartupKind};
use crate::platform::user_path::{self, UserPath};

/// What `floti uninstall` was told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// Whether to remove the data and config folders and the secret store
    /// entry too.
    pub purge: bool,
}

/// What follows `uninstall`: nothing, or `--purge`.
pub fn parse(args: &[String]) -> Option<Options> {
    match args {
        [] => Some(Options { purge: false }),
        [purge] if purge == "--purge" => Some(Options { purge: true }),
        _ => None,
    }
}

/// What stopping the host came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostStop {
    /// No host answered, and no keeper was left running.
    NotRunning,
    /// The host, or the keeper a host left, was stopped and has exited,
    /// ending this many running sessions; `None` where it could not say.
    Stopped { sessions: Option<usize> },
    /// Why it could not be stopped, as one line.
    Failed(String),
}

/// What uninstalling asks of the computer beyond removing files in the
/// folders it is given. [`NativeSystem`] is this computer; a test fakes it.
pub trait UninstallSystem {
    /// Whether this process runs inside one of Floti's own sessions or
    /// plain terminals, which stopping the host would end with it.
    fn inside_session(&self) -> bool;

    /// Ends the processes of `desktop`, the installed desktop app, and
    /// answers whether any were running.
    fn close_desktop(&self, desktop: &Desktop) -> Result<bool, String>;

    /// Stops the host for good, ending its sessions and its keeper, and
    /// waits until it has exited.
    fn stop_host(&self) -> HostStop;

    /// What registering at sign-in runs its commands through.
    fn sign_in(&self) -> &dyn SignInSystem;

    /// Runs the desktop app's own uninstaller `program`, with `line`
    /// appended to its command line as written, and waits for it.
    fn run_uninstaller(&self, program: &Path, line: &str) -> Result<(), String>;

    /// Takes `folder` off the user's `PATH`; whether it was on it.
    fn remove_from_path(&self, folder: &Path) -> Result<bool, String>;

    /// Deletes `files`, then each of `folders` that is empty, once this
    /// process has exited.
    fn delete_after_exit(&self, files: &[PathBuf], folders: &[PathBuf]) -> Result<(), String>;

    /// Removes the OS secret store's `entry`; whether there was one.
    fn remove_secret_entry(&self, entry: &StoreEntry) -> Result<bool, String>;
}

/// What to uninstall, and for whom.
pub struct Uninstall<'a> {
    /// The user's home folder and environment.
    pub env: &'a UserEnv,
    /// Floti's folders for that user.
    pub folders: &'a Folders,
    /// The floti running this.
    pub running: &'a Path,
    /// This OS's start-at-sign-in entry, for its name and where its file is.
    pub definition: &'a StartupDefinition,
    /// The secret store entry this computer's host keeps its key in.
    pub secret_entry: StoreEntry,
    /// Whether to remove the data too.
    pub purge: bool,
}

/// The line a run inside one of Floti's own sessions fails with.
pub const IN_SESSION_LINE: &str = "this terminal is one of Floti's own sessions, and uninstalling stops the host, which ends every session, this one with it; run floti uninstall from a terminal Floti did not start. Nothing was changed.";

/// What a Linux computer with lingering on is told: it is left on.
pub const LINGER_LINE: &str =
    "lingering: left on, since other services may rely on it; loginctl disable-linger turns it off";

/// How long closing the desktop app waits for its processes to go.
const CLOSE_WAIT: Duration = Duration::from_secs(10);

/// How often it looks.
const CLOSE_LOOK: Duration = Duration::from_millis(50);

/// How far up its parents a process is looked at for a session's keeper.
const MOST_PARENTS: usize = 64;

/// The function that stops this computer's host, which the `floti` binary
/// gives: it holds the host's protocol client.
pub type StopHost<'a> = &'a dyn Fn() -> HostStop;

/// Runs `floti uninstall` for this process's user, stopping the host
/// through `stop_host`, and answers its exit status.
pub fn run(options: Options, stop_host: StopHost<'_>) -> u8 {
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    let fail = |err: &mut dyn Write, why: &str| {
        let _ = writeln!(err, "floti uninstall: {why}");
        1
    };
    let env = UserEnv::from_process();
    let running = match std::env::current_exe() {
        Ok(running) => running,
        Err(why) => {
            return fail(
                &mut err,
                &format!("could not tell where this floti is: {why}. Nothing was changed."),
            );
        }
    };
    let folders = match folders::resolve(&env) {
        Ok(folders) => folders,
        Err(why) => return fail(&mut err, &format!("{why} Nothing was changed.")),
    };
    let definition = match startup::Native.definition(&running) {
        Ok(definition) => definition,
        Err(why) => return fail(&mut err, &format!("{why} Nothing was changed.")),
    };
    let secret_entry = if env.var(DATA_DIR_VAR).is_some() {
        StoreEntry::for_moved_data_folder(&folders.data)
    } else {
        StoreEntry::installed()
    };
    let system = NativeSystem {
        env: env.clone(),
        stop_host,
    };
    uninstall(
        &Uninstall {
            env: &env,
            folders: &folders,
            running: &running,
            definition: &definition,
            secret_entry,
            purge: options.purge,
        },
        &standalone::Native,
        &installs::Native,
        &system,
        &mut out,
        &mut err,
    )
}

/// Uninstalls as `what` says, floti's place being `place` and the desktop
/// app's `installs`, asking the rest of `system`, writing a line per step to
/// `out` and a failure to `err`. Answers the exit status: 0 once every step
/// is done, 1 when one failed, and the run stopped there.
pub fn uninstall(
    what: &Uninstall<'_>,
    place: &dyn Standalone,
    installs: &dyn Installs,
    system: &dyn UninstallSystem,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut say = |line: String| {
        let _ = writeln!(out, "{line}").and_then(|()| out.flush());
    };
    match steps(what, place, installs, system, &mut say) {
        Ok(()) => 0,
        Err(why) => {
            let line = why.replace(['\r', '\n'], " ");
            let _ = writeln!(err, "floti uninstall: {line}").and_then(|()| err.flush());
            1
        }
    }
}

type Say<'s> = dyn FnMut(String) + 's;

fn steps(
    what: &Uninstall<'_>,
    place: &dyn Standalone,
    installs: &dyn Installs,
    system: &dyn UninstallSystem,
    say: &mut Say<'_>,
) -> Result<(), String> {
    if system.inside_session() {
        return Err(IN_SESSION_LINE.to_owned());
    }
    if let Some(desktop) = installs.desktop(what.env)
        && system.close_desktop(&desktop).map_err(|why| {
            format!("desktop app: could not close it: {why}. Nothing was removed.")
        })?
    {
        say("desktop app: closed, so it starts no host".to_owned());
    }
    match system.stop_host() {
        HostStop::NotRunning => say("host: not running".to_owned()),
        HostStop::Stopped { sessions } => say(format!("host: stopped, {}", ending(sessions))),
        HostStop::Failed(why) => {
            return Err(format!(
                "host: could not stop it: {why}. Nothing was removed."
            ));
        }
    }
    sign_in_entry(what, system, say)?;
    desktop_app(what, installs, system, say)?;
    match place.binary(what.env) {
        Some(binary) if !there(&binary) || same_file(&binary, what.running) => {
            path_entry(what, place, system, say)?;
            floti_files(what, place, &binary, system, say)?;
        }
        standalone => say(format!(
            "floti: left as it is; this floti runs from {}, not {}, where the install scripts put floti, so it removes only what it installed, and whatever installed it removes it and its PATH entry",
            what.running.display(),
            standalone.map_or_else(
                || "the folder the install scripts use".to_owned(),
                |binary| binary.display().to_string()
            )
        )),
    }
    data(what, system, say)
}

/// What stopping the host ended, after "stopped, ".
pub fn ending(sessions: Option<usize>) -> String {
    match sessions {
        None => "ending every session it held".to_owned(),
        Some(0) => "ending no session".to_owned(),
        Some(1) => "ending 1 session".to_owned(),
        Some(many) => format!("ending {many} sessions"),
    }
}

/// What a person calls `definition`'s entry.
pub fn entry_name(definition: &StartupDefinition) -> String {
    match definition.kind {
        StartupKind::LogonTask => format!("logon task \"{}\"", definition.name),
        StartupKind::LaunchAgent => format!("LaunchAgent {}", definition.name),
        StartupKind::SystemdUserUnit => format!("systemd user unit {}", definition.name),
    }
}

fn sign_in_entry(
    what: &Uninstall<'_>,
    system: &dyn UninstallSystem,
    say: &mut Say<'_>,
) -> Result<(), String> {
    let name = entry_name(what.definition);
    let home = what
        .env
        .require_home()
        .map_err(|why| format!("sign-in entry: {why}"))?;
    let registered =
        register::unregister(system.sign_in(), what.definition, home, &what.folders.data)
            .map_err(|why| format!("sign-in entry: could not remove the {name}: {why}"))?;
    let on_connect = what.folders.data.join(START_ON_CONNECT_FILE);
    let marked = remove_file(&on_connect).map_err(|why| format!("sign-in entry: {why}"))?;
    if registered {
        say(format!("sign-in entry: removed ({name})"));
    }
    if marked {
        say(format!(
            "sign-in entry: removed (the record that has floti connect start the host, {})",
            on_connect.display()
        ));
    }
    if !registered && !marked {
        say("sign-in entry: not found".to_owned());
    }
    if what.definition.kind == StartupKind::SystemdUserUnit
        && register::lingering_on(system.sign_in())
    {
        say(LINGER_LINE.to_owned());
    }
    Ok(())
}

fn desktop_app(
    what: &Uninstall<'_>,
    installs: &dyn Installs,
    system: &dyn UninstallSystem,
    say: &mut Say<'_>,
) -> Result<(), String> {
    let Some(removal) = installs.removal(what.env) else {
        say("desktop app: not found".to_owned());
        return Ok(());
    };
    if let Some((program, line)) = &removal.uninstaller {
        system.run_uninstaller(program, line).map_err(|why| {
            format!(
                "desktop app: its uninstaller {} failed: {why}",
                program.display()
            )
        })?;
    }
    if let Some(gone) = &removal.gone_after
        && there(gone)
    {
        return Err(format!(
            "desktop app: its uninstaller ended, and {} is still there",
            gone.display()
        ));
    }
    for path in &removal.remove {
        remove_tree(&links::Native, path)
            .map_err(|why| format!("desktop app: could not remove {}: {why}", path.display()))?;
    }
    say(format!(
        "desktop app: removed ({})",
        removal.shown.display()
    ));
    Ok(())
}

fn path_entry(
    what: &Uninstall<'_>,
    place: &dyn Standalone,
    system: &dyn UninstallSystem,
    say: &mut Say<'_>,
) -> Result<(), String> {
    let Some(folder) = place.folder(what.env) else {
        say("PATH entry: not found".to_owned());
        return Ok(());
    };
    match system.remove_from_path(&folder) {
        Ok(true) => say(format!("PATH entry: removed ({})", folder.display())),
        Ok(false) => say("PATH entry: not found".to_owned()),
        Err(why) => {
            return Err(format!(
                "PATH entry: could not take {} off your PATH: {why}",
                folder.display()
            ));
        }
    }
    Ok(())
}

/// Removes floti, its Node and what installs left beside it from `binary`'s
/// folder, then the folders the install made for it once they are empty.
/// The Node goes first: a run that stops there leaves floti working.
fn floti_files(
    what: &Uninstall<'_>,
    place: &dyn Standalone,
    binary: &Path,
    system: &dyn UninstallSystem,
    say: &mut Say<'_>,
) -> Result<(), String> {
    let had_binary = there(binary);
    let mut removed = false;
    if let Some(folder) = binary.parent() {
        let node = folder.join(SHIPPED_FOLDER);
        if is_real_folder(&node) {
            remove_tree(&links::Native, &node).map_err(|why| {
                format!(
                    "floti: could not remove its Node, {}: {why}",
                    node.display()
                )
            })?;
            removed = true;
        }
    }
    let mut pending = place.remove_asides(binary);
    if let Some(name) = binary.file_name() {
        remove_copies(binary, &name.to_string_lossy());
    }
    let aside = place
        .remove_running(binary)
        .map_err(|why| format!("floti: could not remove {}: {why}", binary.display()))?;
    pending.extend(aside.iter().cloned());
    let own = place.own_folders(what.env);
    if pending.is_empty() {
        for folder in &own {
            removed |= remove_empty_folder(folder);
        }
    } else {
        system.delete_after_exit(&pending, &own).map_err(|why| {
            format!(
                "floti: removed, but what deletes {} once this floti exits could not start: {why}; floti uninstall run again, or the next floti install or update, removes it",
                pending
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    }
    let left = if aside.is_some() {
        "; the copy this process runs from goes once it exits"
    } else {
        ""
    };
    if had_binary {
        say(format!("floti: removed ({}){left}", binary.display()));
    } else if removed || !pending.is_empty() {
        say(format!(
            "floti: removed what was left of it beside {}",
            binary.display()
        ));
    } else {
        say("floti: not found".to_owned());
    }
    Ok(())
}

fn data(
    what: &Uninstall<'_>,
    system: &dyn UninstallSystem,
    say: &mut Say<'_>,
) -> Result<(), String> {
    let folders = what.folders;
    // Logs and the socket's folder live in the data folder but on Linux.
    let mut named = vec![("data", &folders.data)];
    for (item, folder) in [("logs", &folders.logs), ("runtime", &folders.runtime)] {
        if !folder.starts_with(&folders.data) {
            named.push((item, folder));
        }
    }
    named.push(("config", &folders.config));
    if !what.purge {
        for (item, folder) in named {
            if item == "runtime" {
                continue;
            }
            if there(folder) {
                say(format!(
                    "{item}: kept ({}); floti uninstall --purge removes it",
                    folder.display()
                ));
            } else {
                say(format!("{item}: not found"));
            }
        }
        return Ok(());
    }
    for (item, folder) in named {
        if !there(folder) {
            say(format!("{item}: not found"));
            continue;
        }
        if let Some(why) = refusal(what.env, folder) {
            return Err(format!(
                "{item}: {} was not removed: {why}",
                folder.display()
            ));
        }
        remove_tree(&links::Native, folder)
            .map_err(|why| format!("{item}: could not remove {}: {why}", folder.display()))?;
        say(format!("{item}: removed ({})", folder.display()));
    }
    match system.remove_secret_entry(&what.secret_entry) {
        Ok(true) => say("secret store: removed Floti's key".to_owned()),
        Ok(false) => say("secret store: not found".to_owned()),
        Err(why) => return Err(format!("secret store: {why}")),
    }
    Ok(())
}

/// Why `folder` must never be removed by a purge, or `None` when it may: it
/// is the home folder or holds it, or is, holds or is inside `~/.claude` or
/// `~/.ssh`, whatever an override named.
pub fn refusal(env: &UserEnv, folder: &Path) -> Option<String> {
    let folder = comparable(folder);
    if folder.parent().is_none() {
        return Some("it is the root of a drive".to_owned());
    }
    let home = env.home.as_deref().map(comparable)?;
    if home.starts_with(&folder) {
        return Some("it is your home folder or holds it".to_owned());
    }
    for kept in [".claude", ".ssh"] {
        let kept = home.join(kept);
        if kept.starts_with(&folder) || folder.starts_with(&kept) {
            return Some(format!("it is, holds or is inside {}", kept.display()));
        }
    }
    None
}

/// `path` resolved where it can be, for comparing folders: links followed,
/// so a folder reached through one compares as the folder it is.
fn comparable(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Whether `path` is there, a link included, judged without following it.
fn there(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
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

/// Removes `folder` when it is a real folder and empty; whether it did.
fn remove_empty_folder(folder: &Path) -> bool {
    is_real_folder(folder) && std::fs::remove_dir(folder).is_ok()
}

/// Removes the file `file`; whether it was there.
fn remove_file(file: &Path) -> Result<bool, String> {
    match std::fs::remove_file(file) {
        Ok(()) => Ok(true),
        Err(why) if why.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(why) => Err(format!("could not remove {}: {why}", file.display())),
    }
}

/// Whether the process `this` in `rows`, a process table, has a `floti`
/// among its parents: a session's keeper, or a host running its sessions
/// itself, whose stop ends it. Nothing but those runs a program under a
/// floti a person could type `floti uninstall` into.
pub fn inside_floti(rows: &[ProcessRow], this: u32) -> bool {
    let by_pid: HashMap<u32, &ProcessRow> = rows.iter().map(|row| (row.pid, row)).collect();
    let mut seen = HashSet::from([this]);
    let mut parent = by_pid.get(&this).map(|row| row.parent_pid);
    while let Some(pid) = parent {
        if pid == 0 || !seen.insert(pid) || seen.len() > MOST_PARENTS {
            return false;
        }
        let Some(row) = by_pid.get(&pid) else {
            return false;
        };
        if is_floti(&row.name) {
            return true;
        }
        parent = Some(row.parent_pid);
    }
    false
}

/// Whether a process named `name` is floti.
fn is_floti(name: &str) -> bool {
    name.eq_ignore_ascii_case("floti") || name.eq_ignore_ascii_case("floti.exe")
}

/// This computer, for the user this process runs as.
pub struct NativeSystem<'a> {
    env: UserEnv,
    stop_host: StopHost<'a>,
}

impl UninstallSystem for NativeSystem<'_> {
    fn inside_session(&self) -> bool {
        process_table::Native
            .processes()
            .is_ok_and(|rows| inside_floti(&rows, std::process::id()))
    }

    fn close_desktop(&self, desktop: &Desktop) -> Result<bool, String> {
        let rows = process_table::Native
            .processes()
            .map_err(|why| format!("could not list its processes: {why}"))?;
        let ours: Vec<&ProcessRow> = rows
            .iter()
            .filter(|row| installs::Native.is_desktop_process(desktop, row))
            .collect();
        let pids: HashSet<u32> = ours.iter().map(|row| row.pid).collect();
        let roots: Vec<u32> = ours
            .iter()
            .filter(|row| !pids.contains(&row.parent_pid))
            .map(|row| row.pid)
            .collect();
        if roots.is_empty() {
            return Ok(false);
        }
        let tree = process_tree::Native;
        for pid in &roots {
            tree.end_tree(*pid, Ending::Forced)
                .map_err(|why| why.to_string())?;
        }
        let deadline = Instant::now() + CLOSE_WAIT;
        while roots
            .iter()
            .any(|pid| tree.is_alive(*pid) == Knowable::Known(true))
        {
            if Instant::now() >= deadline {
                return Err(format!(
                    "it was still running {CLOSE_WAIT:?} after it was ended"
                ));
            }
            std::thread::sleep(CLOSE_LOOK);
        }
        Ok(true)
    }

    fn stop_host(&self) -> HostStop {
        (self.stop_host)()
    }

    fn sign_in(&self) -> &dyn SignInSystem {
        &register::NativeSystem
    }

    fn run_uninstaller(&self, program: &Path, line: &str) -> Result<(), String> {
        let mut command = std::process::Command::new(program);
        launcher::Native.append_as_written(&mut command, line);
        launcher::Native.windowless(&mut command);
        let status = command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|why| format!("could not run it: {why}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("it ended with {status}"))
        }
    }

    fn remove_from_path(&self, folder: &Path) -> Result<bool, String> {
        user_path::Native.remove(&self.env, folder)
    }

    fn delete_after_exit(&self, files: &[PathBuf], folders: &[PathBuf]) -> Result<(), String> {
        standalone::Native
            .delete_after_exit(&self.env, files, folders)
            .map_err(|why| why.to_string())
    }

    fn remove_secret_entry(&self, entry: &StoreEntry) -> Result<bool, String> {
        secret_seal::remove_entry(entry)
    }
}
