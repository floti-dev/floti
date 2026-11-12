//! `floti uninstall [--purge]`: takes off this computer what installing
//! Floti put there, either way in (the install scripts, or the desktop
//! installer through `floti install-self`). It is the explicit act, so it
//! asks nothing.
//!
//! In this order, so a run that stops part way leaves nothing half-working,
//! and a run after it, from the same floti, carries on where it stopped:
//!
//! 1. The desktop app's own processes, while they run, are ended one by one
//!    and never as a tree ([`close_processes`]): one that started the host
//!    would start another once the host stops, and the host it started, a
//!    child of the app, is left for step 2 to stop. A process of the app's
//!    that this user cannot end, another user's, is left and said so.
//! 2. The host is stopped for good (`HostStopReason::Stop`), which ends every
//!    session and plain terminal and its session keeper, and says how many
//!    sessions it ended; with no host answering, a keeper a host left running
//!    is ended with what it holds. The sessions' history stays in the data
//!    folder.
//! 3. The start-at-sign-in entry ([`register::unregister`]): the logon task
//!    "Floti host", the LaunchAgent `dev.floti.host` or the user unit
//!    `floti.service`, and in a WSL distro without systemd the record that
//!    makes `floti connect` start the host. Lingering on Linux is a per-user
//!    setting other services may rely on, so it is left on, and said so.
//! 4. The desktop app ([`Installs::removal`]): on Windows by its own NSIS
//!    uninstaller, run silently and waited for, then what it leaves of its
//!    folder, waiting out a file still held for a moment after it exits
//!    ([`held_files::remove_tree`]); the `.app` on macOS; the
//!    AppImage, its menu entry and icon on Linux. The app's own settings are
//!    the app's, kept where the app alone knows, and are left, which one line
//!    says.
//! 5. The `PATH` entry ([`UserPath::remove`]): on Windows the folder's entry
//!    in the user's `Path`. On macOS and Linux floti adds none.
//! 6. The data, with `--purge` (below); without it, a line says what stays.
//!    It comes before floti, so a purge that fails leaves the floti that
//!    runs it to try again.
//! 7. floti and its Node, from where the install scripts put floti
//!    ([`Standalone::folder`]), with the copies an install, an update or an
//!    install script put aside or left part way: install-self's
//!    `.floti.exe.floti-install-*`, the asides `floti.old-*.exe`,
//!    install.ps1's `floti.old.exe`, and a `.floti-<version>.<pid>` download
//!    whose process has ended. A running `floti.exe`
//!    cannot delete itself on Windows, so it is renamed aside and deleted
//!    once this process has exited ([`Standalone::delete_after_exit`]);
//!    where that cannot be started, one line says the copy is left for the
//!    next install's or update's sweep, and the run still succeeds.
//!
//! `--purge` removes the data folder, the logs, the socket's folder where it
//! is apart, and the config folder, unlinking every link in them and never
//! walking into one, then Floti's own entry in the OS secret store. Each
//! folder must be Floti's: the one floti uses with no override
//! ([`Uninstall::layout`]), or, where an override moved it, one holding a
//! file floti writes there ([`marked`]). Before anything is changed it
//! refuses the purge outright when the home folder is unknown, and refuses
//! any folder that is the home folder or holds it, holds a `.git`, or is,
//! holds or is inside `~/.claude` or `~/.ssh` ([`refusal`]); an override's
//! folder with no mark of Floti's is refused the same way, but the socket's
//! folder, which floti marks with nothing, is left and said so.
//!
//! A floti that is not the one where the install scripts put floti (one a
//! package manager installed) removes only what it installed: steps 1 to 4
//! and the data, and says in one line that it leaves itself, and the `PATH`,
//! to whatever installed it.
//!
//! Each step prints one line on standard output, `<what>: <how it went>`,
//! such as `sign-in entry: removed (logon task "Floti host")` or
//! `desktop app: not found`, so a second run says `not found` and exits 0. A
//! step that fails stops the run with one line on standard error,
//! `floti uninstall: <what>: <why>`, and status 1; so does running it from
//! one of Floti's own sessions, which stopping the host would end, before
//! anything is changed.

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use floti_protocol::Knowable;

use crate::connect::START_ON_CONNECT_FILE;
use crate::host_log::LOG_FILE;
use crate::install_self::remove_copies;
use crate::platform::folders::{self, DATA_DIR_VAR, FolderLayout, Folders, UserEnv};
use crate::platform::held_files;
use crate::platform::installs::{self, Desktop, Installs};
use crate::platform::launcher::{self, ProgramLauncher};
use crate::platform::links::{self, remove_tree};
use crate::platform::node_runtime::SHIPPED_FOLDER;
use crate::platform::process_table::{self, ProcessRow, ProcessTable};
use crate::platform::process_tree::{self, OneProcess, Outliving, ProcessTree};
use crate::platform::secret_seal::{self, StoreEntry};
use crate::platform::standalone::{self, Standalone};
use crate::platform::startup::register::{self, SignInSystem};
use crate::platform::startup::{self, StartAtSignIn, StartupDefinition, StartupKind};
use crate::platform::user_path::{self, UserPath};
use crate::server::HOST_KEY_FILE;
use crate::store::DB_FILE;
use crate::update::leftover_pid;

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

/// What closing the desktop app did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Closed {
    /// How many of its processes were ended.
    pub ended: usize,
    /// How many were left because this user cannot end them: another
    /// user's.
    pub skipped: usize,
}

/// What uninstalling asks of the computer beyond removing files in the
/// folders it is given. [`NativeSystem`] is this computer; a test fakes it.
pub trait UninstallSystem {
    /// Whether this process runs inside one of Floti's own sessions or
    /// plain terminals, which stopping the host would end with it.
    fn inside_session(&self) -> bool;

    /// Ends the processes of `desktop`, the installed desktop app, each
    /// alone ([`close_processes`]).
    fn close_desktop(&self, desktop: &Desktop) -> Result<Closed, String>;

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
    /// process has exited, and says where what does it runs.
    fn delete_after_exit(
        &self,
        files: &[PathBuf],
        folders: &[PathBuf],
    ) -> Result<Outliving, String>;

    /// Removes the OS secret store's `entry`; whether there was one.
    fn remove_secret_entry(&self, entry: &StoreEntry) -> Result<bool, String>;

    /// Whether process `pid` is known to have ended.
    fn ended(&self, pid: u32) -> bool;
}

/// What to uninstall, and for whom.
pub struct Uninstall<'a> {
    /// The user's home folder and environment.
    pub env: &'a UserEnv,
    /// Floti's folders for that user, overrides applied.
    pub folders: &'a Folders,
    /// The folders floti uses for that user with no override, where the
    /// OS's layout can say: a purge removes these without looking for a
    /// mark of Floti's.
    pub layout: Option<Folders>,
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

/// What the floti line adds when what deletes floti once it exits had to
/// start in a job that ends what it holds when it closes.
pub const JOB_ENDS_IT_FIRST: &str = ", unless the job this floti runs in, which ends every process in it when it closes, ends that first";

/// What a Linux computer with lingering on is told: it is left on.
pub const LINGER_LINE: &str =
    "lingering: left on, since other services may rely on it; loginctl disable-linger turns it off";

/// What is said of the desktop app's own settings, once it is removed or
/// with `--purge`: they are the app's, and stay.
pub const DESKTOP_SETTINGS_LINE: &str = "desktop app settings: kept; they are the desktop app's own, where it keeps them, and floti uninstall leaves them";

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
            layout: folders::Native.resolve(&env).ok(),
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
    let purge = if what.purge {
        Some(purge_plan(what)?)
    } else {
        None
    };
    if let Some(desktop) = installs.desktop(what.env) {
        let closed = system.close_desktop(&desktop).map_err(|why| {
            format!("desktop app: could not close it: {why}. Nothing was removed.")
        })?;
        if closed.ended > 0 {
            say("desktop app: closed, so it starts no host".to_owned());
        }
        if closed.skipped > 0 {
            say(format!(
                "desktop app: left {} running that this user cannot end, such as another user's",
                processes(closed.skipped)
            ));
        }
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
    let desktop_removed = desktop_app(what, installs, system, say)?;
    let standalone = place.binary(what.env);
    let ours = standalone
        .as_deref()
        .is_some_and(|binary| binary == what.running || same_file(binary, what.running));
    if ours {
        path_entry(what, place, system, say)?;
    }
    match &purge {
        Some(plan) => {
            if !desktop_removed {
                say(DESKTOP_SETTINGS_LINE.to_owned());
            }
            purge_data(what, plan, system, say)?;
        }
        None => keep_data(what, say),
    }
    match standalone {
        Some(binary) if ours => floti_files(what, place, &binary, system, say)?,
        _ => say(format!(
            "floti: left as it is (installed by {}); it removes only what it installed, and leaves itself and its PATH entry to whatever installed it",
            what.running.display()
        )),
    }
    Ok(())
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

/// `count` processes, in words.
fn processes(count: usize) -> String {
    if count == 1 {
        "1 process".to_owned()
    } else {
        format!("{count} processes")
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

/// Ends one process of the desktop app's.
pub trait ProcessEnder {
    /// Ends process `pid` alone, at once, never its tree. A refusal because
    /// this user may not end it is `PermissionDenied`.
    fn end(&self, pid: u32) -> io::Result<()>;

    /// Whether process `pid` is still running, as far as can be told.
    fn alive(&self, pid: u32) -> bool;
}

/// Ends each process of `rows` that `is_app` says is the desktop app's, one
/// by one through `ender`, and never as a tree: the host a desktop started
/// is its child, and is the host stop's to end, which counts its sessions
/// and ends its keeper the deliberate way. One this user may not end
/// (another user's) is left and counted; one that had already gone is not
/// counted at all. Then waits, through `pause`, up to `patience` for those
/// it ended to go.
pub fn close_processes(
    rows: &[ProcessRow],
    is_app: &dyn Fn(&ProcessRow) -> bool,
    ender: &dyn ProcessEnder,
    patience: Duration,
    pause: &mut dyn FnMut(Duration),
) -> Result<Closed, String> {
    let mut ended = Vec::new();
    let mut skipped = 0;
    for row in rows.iter().filter(|row| is_app(row)) {
        match ender.end(row.pid) {
            Ok(()) => ended.push(row.pid),
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => skipped += 1,
            Err(_) if !ender.alive(row.pid) => {}
            Err(err) => return Err(format!("could not end its process {}: {err}", row.pid)),
        }
    }
    let mut waited = Duration::ZERO;
    while ended.iter().any(|pid| ender.alive(*pid)) {
        if waited >= patience {
            return Err(format!(
                "it was still running {patience:?} after it was ended"
            ));
        }
        pause(CLOSE_LOOK);
        waited += CLOSE_LOOK;
    }
    Ok(Closed {
        ended: ended.len(),
        skipped,
    })
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

/// Removes the desktop app, and answers whether it was there to remove.
fn desktop_app(
    what: &Uninstall<'_>,
    installs: &dyn Installs,
    system: &dyn UninstallSystem,
    say: &mut Say<'_>,
) -> Result<bool, String> {
    let Some(removal) = installs.removal(what.env) else {
        say("desktop app: not found".to_owned());
        return Ok(false);
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
    // The uninstaller has exited (`run_uninstaller` waits for it), but its
    // own exe can stay held for a moment after, by Windows or an antivirus
    // scan, so a held file is waited out before the removal fails.
    for path in &removal.remove {
        held_files::remove_tree(&links::Native, path)
            .map_err(|why| format!("desktop app: could not remove {}: {why}", path.display()))?;
    }
    say(format!(
        "desktop app: removed ({})",
        removal.shown.display()
    ));
    say(DESKTOP_SETTINGS_LINE.to_owned());
    Ok(true)
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

/// Removes floti, the Node install-self put beside it, and what installs
/// and install scripts left beside it, from `binary`'s folder, then the
/// folders the install made for it once they are empty. The Node goes
/// first: a run that stops there leaves floti working.
fn floti_files(
    what: &Uninstall<'_>,
    place: &dyn Standalone,
    binary: &Path,
    system: &dyn UninstallSystem,
    say: &mut Say<'_>,
) -> Result<(), String> {
    let had_binary = there(binary);
    let mut removed = false;
    // Only beside a floti installed on its own is a `node` folder Floti's.
    if had_binary && let Some(folder) = binary.parent() {
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
    pending.extend(remove_script_leftovers(binary, system));
    if let Some(name) = binary.file_name() {
        remove_copies(binary, &name.to_string_lossy());
    }
    let aside = place
        .remove_running(binary)
        .map_err(|why| format!("floti: could not remove {}: {why}", binary.display()))?;
    pending.extend(aside.iter().cloned());
    let own = place.own_folders(what.env);
    let mut left = String::new();
    if pending.is_empty() {
        for folder in &own {
            removed |= remove_empty_folder(folder);
        }
    } else {
        let named = pending
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        match system.delete_after_exit(&pending, &own) {
            Ok(outliving) => {
                let unless = match outliving {
                    Outliving::InJob {
                        ends_with_job: true,
                    } => JOB_ENDS_IT_FIRST,
                    _ => "",
                };
                if aside.is_some() {
                    left = format!("; the copy this process runs from goes once it exits{unless}");
                } else if !unless.is_empty() {
                    left = format!("; {named} goes once this floti exits{unless}");
                }
            }
            Err(why) => {
                left = format!(
                    "; {named} is left for the next floti install or update to remove, since what would delete it once this floti exits could not start ({why})"
                );
            }
        }
    }
    if had_binary {
        say(format!("floti: removed ({}){left}", binary.display()));
    } else if removed || !pending.is_empty() {
        say(format!(
            "floti: removed what was left of it beside {}{left}",
            binary.display()
        ));
    } else {
        say("floti: not found".to_owned());
    }
    Ok(())
}

/// Removes what an install script or an update left beside `binary` when
/// it stopped part way, and answers what is still to delete once this floti
/// exits: install.ps1's copy put aside (`floti.old.exe`), which fails to go
/// only while something runs it, and a `.floti-<version>.<pid>` download
/// (install.ps1's, install.sh's or `floti update`'s, [`leftover_pid`]) whose
/// process has ended. A download whose process still runs is that process's,
/// and is left.
fn remove_script_leftovers(binary: &Path, system: &dyn UninstallSystem) -> Vec<PathBuf> {
    let (Some(folder), Some(stem)) = (binary.parent(), binary.file_stem()) else {
        return Vec::new();
    };
    let extension = binary
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let script_aside = format!("{}.old{extension}", stem.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut left = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name == script_aside {
            if std::fs::remove_file(&path).is_err() {
                left.push(path);
            }
        } else if leftover_pid(&name, &extension).is_some_and(|pid| system.ended(pid)) {
            let _ = std::fs::remove_file(&path);
        }
    }
    left
}

/// One folder a purge looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgeTarget {
    /// What its lines call it: `data`, `logs`, `runtime` or `config`.
    pub item: &'static str,
    pub folder: PathBuf,
    pub action: PurgeAction,
}

/// What a purge does with one folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurgeAction {
    /// It is Floti's, and goes.
    Remove,
    /// It is not there.
    NotThere,
    /// It is left, for this reason: the socket's folder an override moved,
    /// which floti marks with nothing.
    Leave(String),
}

/// The folders `what`'s purge looks at, each with what it does, decided
/// before anything is changed. `Err` refuses the whole purge: the home
/// folder is unknown, or a folder must never be removed ([`refusal`]), or
/// is an override's with no mark of Floti's ([`marked`]).
pub fn purge_plan(what: &Uninstall<'_>) -> Result<Vec<PurgeTarget>, String> {
    let nothing = "Nothing was changed; floti uninstall without --purge removes the rest";
    let Some(home) = what.env.home.as_deref() else {
        return Err(format!(
            "--purge needs your home folder, to tell what it must never remove, and none is known. {nothing}."
        ));
    };
    let folders = what.folders;
    let layout = what.layout.as_ref();
    // Logs and the socket's folder live in the data folder but on Linux.
    let mut named: Vec<(&'static str, &PathBuf, Option<&PathBuf>)> =
        vec![("data", &folders.data, layout.map(|layout| &layout.data))];
    for (item, folder, usual) in [
        ("logs", &folders.logs, layout.map(|layout| &layout.logs)),
        (
            "runtime",
            &folders.runtime,
            layout.map(|layout| &layout.runtime),
        ),
    ] {
        if !folder.starts_with(&folders.data) {
            named.push((item, folder, usual));
        }
    }
    named.push((
        "config",
        &folders.config,
        layout.map(|layout| &layout.config),
    ));
    named
        .into_iter()
        .map(|(item, folder, usual)| {
            let action = if !there(folder) {
                PurgeAction::NotThere
            } else if let Some(why) = refusal(home, folder) {
                return Err(format!(
                    "{item}: {} must never be removed: {why}. {nothing}.",
                    folder.display()
                ));
            } else if usual.is_some_and(|usual| comparable(usual) == comparable(folder))
                || marked(item, folder)
            {
                PurgeAction::Remove
            } else if item == "runtime" {
                PurgeAction::Leave("it is not the folder floti uses".to_owned())
            } else {
                return Err(format!(
                    "{item}: {} is not the folder floti uses and holds nothing floti writes there, so it was not removed. {nothing}.",
                    folder.display()
                ));
            };
            Ok(PurgeTarget {
                item,
                folder: folder.clone(),
                action,
            })
        })
        .collect()
}

/// Whether `folder`, the purge's `item`, holds a file floti writes there:
/// the data folder its host key or `floti.db`, the logs the host's log, the
/// config folder the `agents` or `templates` folder floti seeds.
pub fn marked(item: &str, folder: &Path) -> bool {
    let has_file = |name: &str| folder.join(name).is_file();
    let has_folder = |name: &str| is_real_folder(&folder.join(name));
    match item {
        "data" => has_file(HOST_KEY_FILE) || has_file(DB_FILE),
        "logs" => has_file(LOG_FILE),
        "config" => has_folder("agents") || has_folder("templates"),
        _ => false,
    }
}

/// Why `folder` must never be removed by a purge for the user whose home
/// folder is `home`, or `None` when it may: it is a root, the home folder or
/// holds it, holds a `.git` (a repository), or is, holds or is inside
/// `~/.claude` or `~/.ssh`, whatever an override named.
pub fn refusal(home: &Path, folder: &Path) -> Option<String> {
    if folder.join(".git").exists() {
        return Some("it holds a .git, so it is a repository".to_owned());
    }
    let folder = comparable(folder);
    if folder.parent().is_none() {
        return Some("it is the root of a drive".to_owned());
    }
    let home = comparable(home);
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

/// Says what stays without `--purge`.
fn keep_data(what: &Uninstall<'_>, say: &mut Say<'_>) {
    let folders = what.folders;
    let mut named = vec![("data", &folders.data)];
    if !folders.logs.starts_with(&folders.data) {
        named.push(("logs", &folders.logs));
    }
    named.push(("config", &folders.config));
    for (item, folder) in named {
        if there(folder) {
            say(format!(
                "{item}: kept ({}); floti uninstall --purge removes it",
                folder.display()
            ));
        } else {
            say(format!("{item}: not found"));
        }
    }
}

fn purge_data(
    what: &Uninstall<'_>,
    plan: &[PurgeTarget],
    system: &dyn UninstallSystem,
    say: &mut Say<'_>,
) -> Result<(), String> {
    for target in plan {
        let (item, folder) = (target.item, &target.folder);
        match &target.action {
            PurgeAction::NotThere => say(format!("{item}: not found")),
            PurgeAction::Leave(why) => {
                say(format!(
                    "{item}: left as it is ({}); {why}",
                    folder.display()
                ));
            }
            PurgeAction::Remove => {
                remove_tree(&links::Native, folder).map_err(|why| {
                    format!("{item}: could not remove {}: {why}", folder.display())
                })?;
                say(format!("{item}: removed ({})", folder.display()));
            }
        }
    }
    match system.remove_secret_entry(&what.secret_entry) {
        Ok(true) => say("secret store: removed Floti's key".to_owned()),
        Ok(false) => say("secret store: not found".to_owned()),
        Err(why) => return Err(format!("secret store: {why}")),
    }
    Ok(())
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

/// This computer's processes, each ended alone ([`OneProcess`]): on
/// Windows by a handle, which another user's process does not give.
struct NativeEnder;

impl ProcessEnder for NativeEnder {
    fn end(&self, pid: u32) -> io::Result<()> {
        OneProcess::hold(pid)?.end_now()
    }

    fn alive(&self, pid: u32) -> bool {
        process_tree::Native.is_alive(pid) == Knowable::Known(true)
    }
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

    fn close_desktop(&self, desktop: &Desktop) -> Result<Closed, String> {
        let rows = process_table::Native
            .processes()
            .map_err(|why| format!("could not list its processes: {why}"))?;
        close_processes(
            &rows,
            &|row| installs::Native.is_desktop_process(desktop, row),
            &NativeEnder,
            CLOSE_WAIT,
            &mut std::thread::sleep,
        )
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

    fn delete_after_exit(
        &self,
        files: &[PathBuf],
        folders: &[PathBuf],
    ) -> Result<Outliving, String> {
        standalone::Native
            .delete_after_exit(&self.env, files, folders)
            .map_err(|why| why.to_string())
    }

    fn remove_secret_entry(&self, entry: &StoreEntry) -> Result<bool, String> {
        secret_seal::remove_entry(entry)
    }

    fn ended(&self, pid: u32) -> bool {
        process_tree::Native.is_alive(pid) == Knowable::Known(false)
    }
}
