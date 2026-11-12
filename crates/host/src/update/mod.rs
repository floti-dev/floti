//! `floti update [--channel <name>]`: replaces the running floti with its
//! channel's newest release, has this computer's host start it in its
//! place, and updates the desktop app where it is installed and older, once
//! the app has quit. The host runs the same update in the background, for
//! floti alone ([`background`]).
//!
//! 1. It removes what an earlier update or install left beside the running
//!    floti: copies put aside (`floti.old-<pid>-<n>.exe`), where nothing
//!    runs them any more (`Standalone::remove_asides`), and downloads
//!    (`.floti-<version>.<pid>[.exe]`) and what it left beside the desktop
//!    app (`.Helm.app.floti-old-<pid>` and the like), where the process that
//!    left it is known to have ended.
//! 2. It downloads `https://floti.dev/update.json` and its signature
//!    `update.json.minisig`, and checks the signature with the key built
//!    into the binary and its trusted comment, `floti update.json`
//!    ([`signature`]). The channel is the one `--channel` names, which is
//!    then remembered ([`channel`]), or the one remembered, `stable` at
//!    first. A channel the manifest does not have is an error naming it.
//! 3. It decides what to replace. floti, when the channel's version is newer
//!    than this one's; a version this floti cannot read its own version as
//!    stops the update, so an older or equal version is never installed. A
//!    floti that is part of the desktop app's install is never replaced on
//!    its own: the app's installer brings its own, and it says so. The app,
//!    when it is installed and its own version is older than the channel's
//!    or cannot be told, so an app behind an up-to-date floti is still
//!    updated. It asks nothing: installing either ends no session. With
//!    nothing to replace, it says floti is up to date and changes nothing.
//! 4. It downloads this computer's floti beside the running one, under a
//!    name of its own, and the app's release file into `update` in the data
//!    folder, each checked against its signature and its SHA-256
//!    ([`release_file`]).
//! 5. Only when everything has passed does it replace anything: floti
//!    first ([`Standalone::replace`], as `floti install-self` puts it in
//!    place), and this computer's host is asked to start the new floti in
//!    its place ([`HostStopReason::Relaunch`]): it lets go of its session
//!    keeper, so every session and plain terminal runs on, and the new host
//!    takes them back. Then, once no process of the app is left, the app
//!    ([`Installs::install_desktop`]), and then, where the app's own
//!    installer does not, `floti install-self` from the new app's floti
//!    ([`Installs::carried_floti`]), so the Node beside floti is the new
//!    app's. While the app
//!    runs it says it is waiting for it to quit, and looks again every
//!    second.
//!
//! Any failed check stops the update with one line on standard error that
//! names the file and what failed, and nothing is replaced; what was
//! downloaded is removed. Progress goes to standard output, one line a step.
//! Exit status 0 when floti is up to date or updated, 1 otherwise.
//!
//! One update runs at a time on a computer: each takes the run file in the
//! data folder first and refuses while another holds it ([`run_file`]),
//! and says each step there too, marked while it waits for the app, so a
//! host can follow an update it did not start. How it ended is kept beside.
//!
//! `floti update --from-desktop` is what the desktop's Update runs, through
//! its host (`update:install`). A desktop on this computer quits when the
//! run file says the update waits for it, its sessions running on. The host
//! is asked to start the new floti only once the app is replaced, and the
//! app that quit for it is opened again ([`Installs::opener`]), replaced or,
//! when that failed, as it was, with the display and session-bus variables
//! it ran with ([`Installs::display_env`]).
//!
//! What is checked and replaced is the per-OS concern
//! [`platform::installs`](crate::platform::installs).
//!
//! [`HostStopReason::Relaunch`]: floti_protocol::channels::host::HostStopReason::Relaunch

pub mod background;
pub mod channel;
pub mod manifest;
pub mod network;
pub mod release_file;
pub mod run_file;
pub mod signature;
pub mod versions;

#[cfg(test)]
mod tests;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use floti_protocol::Knowable;
use jiff::Timestamp;
use minisign_verify::PublicKey;
use url::Url;

use crate::platform::folders::{self, UserEnv};
use crate::platform::installs::{self, Desktop, Installs};
use crate::platform::links::{self, remove_tree};
use crate::platform::process_table::{self, ProcessTable};
use crate::platform::process_tree::{self, ProcessTree};
use crate::platform::standalone::{self, Standalone};
use crate::version::{FLOTI_VERSION, cpu_name, os_name};
use manifest::{FileEntry, Release};
use network::{Network, get_whole};
use release_file::{fetch_refusal, fetch_signature};
use run_file::RunLock;
use signature::{MANIFEST_COMMENT, check_whole};
use versions::Version;

/// Where the manifest is.
pub const MANIFEST_URL: &str = "https://floti.dev/update.json";

/// The manifest's file name, as its signature and errors name it.
const MANIFEST_FILE: &str = "update.json";

/// The folder in the data folder a desktop download is kept in until it is
/// installed.
pub const STAGING_FOLDER: &str = "update";

/// How often a running desktop app is looked for again.
pub const APP_POLL: Duration = Duration::from_secs(1);

const MANIFEST_LIMIT: u64 = 1024 * 1024;

/// What went wrong, as a fact. `floti update` adds what that means for the
/// update ([`failure_line`]).
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("could not download {file} from {url}: {reason}.")]
    Network {
        file: String,
        url: String,
        reason: String,
    },
    #[error("{file} has no signature ({file}.minisig is not there).")]
    MissingSignature { file: String },
    #[error("{file} does not match its signature.")]
    BadSignature { file: String },
    #[error("{file} is not signed with the key built into floti.")]
    WrongKey { file: String },
    #[error("the signature of {file} is for \"{found}\", not \"{wanted}\".")]
    WrongComment {
        file: String,
        found: String,
        wanted: String,
    },
    #[error("{file} does not match its SHA-256 in update.json.")]
    WrongSha256 { file: String },
    #[error("the download of {file} stopped before its end ({reason}).")]
    CutOff { file: String, reason: String },
    #[error("{0}")]
    Manifest(String),
    #[error("update.json has no channel \"{channel}\"; it has {}.", known.join(", "))]
    NoChannel { channel: String, known: Vec<String> },
    #[error("floti {version} on {channel} has no build for {target}.")]
    NoBuild {
        channel: String,
        version: String,
        target: String,
    },
    #[error(
        "this floti's own version, \"{version}\", is not a version, so floti cannot tell which release is newer."
    )]
    RunningVersion { version: String },
    #[error("{reason}.")]
    Disk { reason: String },
    #[error("another floti update is running (process {pid})")]
    AlreadyRunning { pid: u32 },
    #[error("floti could not be replaced: {reason}")]
    Replace { reason: String },
    /// floti was replaced; the app was not.
    #[error("floti is updated to {version}, but {app} is not: {reason}")]
    Desktop {
        version: String,
        app: String,
        reason: String,
    },
    /// Only the app was to be updated, and it was not.
    #[error("{app} could not be updated to {version}: {reason}")]
    DesktopAlone {
        version: String,
        app: String,
        reason: String,
    },
}

/// The line `floti update` prints for `err`: the fact, then that nothing
/// was updated, unless floti already was.
pub fn failure_line(err: &UpdateError) -> String {
    let line = err.to_string();
    match err {
        UpdateError::Desktop { .. } => line,
        _ => nothing_updated(&line),
    }
}

/// `line` as a sentence, then "Nothing was updated."
fn nothing_updated(line: &str) -> String {
    let stop = if line.ends_with('.') { "" } else { "." };
    format!("{line}{stop} Nothing was updated.")
}

/// How an update ended well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The channel has nothing newer than this floti and the app.
    UpToDate { channel: String, newest: String },
    /// floti was replaced when `floti` is true, and the desktop app when
    /// `desktop` names it.
    Updated {
        version: String,
        floti: bool,
        desktop: Option<String>,
    },
}

/// How asking this computer's host to start the new floti in its place went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRelaunch {
    /// No host runs here, so there was nothing to ask.
    NoHost,
    /// The host let go of its sessions and is starting the new floti.
    Relaunched,
    /// The host could not be asked, with why.
    Failed(String),
}

/// Asks this computer's host to start the floti now in its place.
pub type RelaunchHost<'a> = &'a dyn Fn() -> HostRelaunch;

/// Opens the desktop app with display and session-bus variables, or says
/// why it could not.
pub type OpenDesktop<'a> = &'a dyn Fn(&Desktop, &[(String, String)]) -> Result<(), String>;

/// What an update the desktop's Update started does besides the rest
/// (`floti update --from-desktop`).
pub struct FromDesktop<'a> {
    /// Opens the desktop app again, with the display and session-bus
    /// variables it ran with, or says why it could not.
    pub open: OpenDesktop<'a>,
}

/// The first line an update says, in the run file, before anything else.
pub const STARTING_LINE: &str = "Starting floti update.";

/// The last line of an update that ended well.
pub const FINISHED_LINE: &str = "floti update has finished.";

/// What `floti update` was asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// The channel `--channel` chose, or `None` for the remembered one.
    pub channel: Option<String>,
    /// Whether the desktop's Update started it (`--from-desktop`).
    pub from_desktop: bool,
}

/// What one update runs with: every piece of the machine it touches, so a
/// test can hand it fakes.
pub struct Updater<'a> {
    /// The version running now.
    pub running_version: &'a str,
    /// This computer, as release files name it: `windows-x64`.
    pub target: String,
    pub manifest_url: Url,
    pub key: &'a PublicKey,
    pub network: &'a dyn Network,
    pub installs: &'a dyn Installs,
    /// Where floti installed on its own is, and how a file there is put in
    /// place.
    pub standalone: &'a dyn Standalone,
    /// Runs `<floti> install-self` and answers what it printed, or its
    /// failure line.
    pub install_self: &'a dyn Fn(&Path) -> Result<String, String>,
    pub processes: &'a dyn ProcessTable,
    /// Whether a process is alive, for what earlier updates left.
    pub is_alive: &'a dyn Fn(u32) -> Knowable<bool>,
    /// Asks this computer's host to start the new floti in its place once
    /// floti is replaced; `None` where the host itself is updating, which
    /// relaunches itself after.
    pub relaunch_host: Option<RelaunchHost<'a>>,
    /// Whether the desktop app is updated too. The host's background
    /// update leaves it.
    pub with_desktop: bool,
    /// Waits, between looks at a running app.
    pub pause: &'a dyn Fn(Duration),
    /// The user, for where the desktop app is.
    pub env: UserEnv,
    /// The floti being replaced.
    pub running_binary: PathBuf,
    /// Floti's data folder: the remembered channel, and the staging folder.
    pub data: PathBuf,
    /// The channel `--channel` chose, or `None` for the remembered one.
    pub chosen_channel: Option<String>,
    /// Set when the desktop's Update started it: progress as JSON, the host
    /// asked to start the new floti once the app is replaced, and the app
    /// opened again.
    pub from_desktop: Option<FromDesktop<'a>>,
    /// Where progress lines go.
    pub out: &'a mut dyn Write,
    /// The run file while the update runs: `None` where an updater is made.
    held: Option<RunLock>,
}

impl Updater<'_> {
    /// Runs the update, holding the run file throughout, and keeps how it
    /// ended. Refused, changing nothing, while another update runs.
    pub fn run(&mut self) -> Result<Outcome, UpdateError> {
        self.held = Some(RunLock::take(
            &self.data,
            std::process::id(),
            STARTING_LINE,
            self.is_alive,
        )?);
        let outcome = self.run_held();
        if let Some(held) = self.held.take() {
            let line = outcome.as_ref().err().map(failure_line);
            held.end(
                Timestamp::now(),
                line.as_deref().map_or(Ok(FINISHED_LINE), Err),
            );
        }
        outcome
    }

    fn run_held(&mut self) -> Result<Outcome, UpdateError> {
        self.sweep_leftovers();
        let running =
            Version::parse(self.running_version).ok_or_else(|| UpdateError::RunningVersion {
                version: self.running_version.to_owned(),
            })?;
        let channel = self
            .chosen_channel
            .clone()
            .unwrap_or_else(|| channel::remembered(&self.data));
        let manifest = self.manifest()?;
        let release = manifest::release(&manifest, &channel, &self.target)?;
        if self.chosen_channel.is_some() {
            channel::remember(&self.data, &channel)?;
        }
        let version = release.version().clone();

        let installed = self
            .installs
            .desktop(&self.env)
            .filter(|_| self.with_desktop);
        let bundled_in = installed
            .clone()
            .filter(|app| self.installs.bundles(app, &self.running_binary));
        if let Some(app) = &bundled_in {
            self.say_bundled(app);
        }
        let floti = if bundled_in.is_none() && version > running {
            Some(release.floti.clone().ok_or_else(|| UpdateError::NoBuild {
                channel: channel.clone(),
                version: version.to_string(),
                target: self.target.clone(),
            })?)
        } else {
            None
        };
        let desktop = self.desktop_step(installed, &release);
        if floti.is_none() && desktop.is_none() {
            self.say(&format!(
                "floti {} is up to date on {channel} (its newest is {version}).",
                self.running_version
            ));
            return Ok(Outcome::UpToDate {
                channel,
                newest: version.to_string(),
            });
        }

        let staging = self.data.join(STAGING_FOLDER);
        clear(&staging).map_err(|reason| UpdateError::Disk { reason })?;
        let new_binary = floti.as_ref().map(|_| self.beside_running(&version));
        let checked = self.download_all(
            &release,
            floti.as_ref().zip(new_binary.as_deref()),
            desktop.as_ref().map(|(_, entry)| entry),
            &staging,
        );
        let discard = |new_binary: &Option<PathBuf>| {
            if let Some(path) = new_binary {
                let _ = std::fs::remove_file(path);
            }
            let _ = clear(&staging);
        };
        if let Err(err) = checked {
            discard(&new_binary);
            return Err(err);
        }

        if let Some(new) = &new_binary {
            if let Err(err) = self.standalone.replace(&self.running_binary, new) {
                discard(&new_binary);
                return Err(UpdateError::Replace {
                    reason: err.to_string(),
                });
            }
            self.say(&format!(
                "floti is updated from {} to {version}.",
                self.running_version
            ));
            if self.from_desktop.is_none() {
                self.relaunch_host(&version);
            }
        }

        // From the desktop, the host that started this hears it to its end:
        // it is asked to start the new floti once the app is replaced, and
        // before the app that quit for it opens again.
        let relaunch_after_app = self.from_desktop.is_some() && new_binary.is_some();
        let updated_desktop = match desktop {
            Some((app, entry)) => {
                let mut waited = None;
                let result = self.install_desktop(
                    &app,
                    &staging.join(&entry.file),
                    &version,
                    new_binary.is_some(),
                    &mut waited,
                );
                let _ = clear(&staging);
                if relaunch_after_app {
                    self.relaunch_host(&version);
                }
                if let Some(display) = waited {
                    self.open_desktop(&app, &display);
                }
                result?;
                Some(app.name.to_owned())
            }
            None => {
                let _ = clear(&staging);
                if relaunch_after_app {
                    self.relaunch_host(&version);
                }
                None
            }
        };
        Ok(Outcome::Updated {
            version: version.to_string(),
            floti: new_binary.is_some(),
            desktop: updated_desktop,
        })
    }

    fn say(&mut self, line: &str) {
        self.report(line, false);
    }

    /// Says `line`, and keeps it in the run file, marked as waiting for the
    /// app when `waiting_for_app`.
    fn report(&mut self, line: &str, waiting_for_app: bool) {
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
        if let Some(held) = &mut self.held {
            held.step(line, waiting_for_app);
        }
    }

    /// Opens `app` again with the variables `display`, when the desktop
    /// started the update, and says how that went.
    fn open_desktop(&mut self, app: &Desktop, display: &[(String, String)]) {
        let Some(open) = self.from_desktop.as_ref().map(|from| from.open) else {
            return;
        };
        match open(app, display) {
            Ok(()) => self.say(&format!("Opening {} again.", app.name)),
            Err(why) => self.say(&format!(
                "{} could not be opened again ({why}); open it yourself.",
                app.name
            )),
        }
    }

    /// Says that this floti is `app`'s, and where floti installed on its
    /// own is, when it is there.
    fn say_bundled(&mut self, app: &Desktop) {
        let mut line = format!(
            "This floti is part of {} ({}), so it is updated with {}, never on its own.",
            app.name,
            app.path.display(),
            app.name
        );
        if let Some(standalone) = self
            .standalone
            .binary(&self.env)
            .filter(|path| path.is_file() && *path != self.running_binary)
        {
            line.push_str(&format!(
                " floti installed on its own is {0}; `{0} update` updates it.",
                standalone.display()
            ));
        }
        self.say(&line);
    }

    /// Removes what an earlier update left beside the running floti and
    /// beside the desktop app, where the process that left it has ended.
    /// Nothing it cannot remove stops this update.
    fn sweep_leftovers(&self) {
        self.sweep_beside_binary();
        self.sweep_beside_desktop();
    }

    /// Whether the process `pid` is known to have ended.
    fn ended(&self, pid: u32) -> bool {
        matches!((self.is_alive)(pid), Knowable::Known(false))
    }

    /// The copies put aside and the downloads left beside floti.
    fn sweep_beside_binary(&self) {
        self.standalone.remove_asides(&self.running_binary);
        let Some(folder) = self.running_binary.parent() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        let extension = self.extension();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name
                .to_str()
                .and_then(|name| leftover_pid(name, &extension))
            else {
                continue;
            };
            if self.ended(pid) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// The new and old copies of the app an install ended before removing
    /// ([`installs::leftovers`]): an unpacked `.app` or a written AppImage
    /// on macOS and Linux. A copy is a folder on macOS, removed without
    /// walking into a link.
    fn sweep_beside_desktop(&self) {
        let Some(app) = self.installs.desktop_target(&self.env) else {
            return;
        };
        for (path, pid) in installs::leftovers(&app) {
            if self.ended(pid) {
                let _ = remove_tree(&links::Native, &path);
            }
        }
    }

    /// The running binary's extension with its dot, or nothing.
    fn extension(&self) -> String {
        self.running_binary
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default()
    }

    /// The installed app and its release file, when the app is older than
    /// the release or its version cannot be told, and the release has one
    /// for this computer.
    fn desktop_step(
        &mut self,
        installed: Option<Desktop>,
        release: &Release,
    ) -> Option<(Desktop, FileEntry)> {
        let app = installed?;
        let current = self
            .installs
            .desktop_version(&app)
            .and_then(|version| Version::parse(&version));
        if current.is_some_and(|current| current >= *release.version()) {
            return None;
        }
        match &release.desktop {
            Some(entry) => Some((app, entry.clone())),
            None => {
                self.say(&format!(
                    "floti {} has no {} for {}, so {} stays as it is.",
                    release.version(),
                    app.name,
                    self.target,
                    app.name
                ));
                None
            }
        }
    }

    /// Asks this computer's host, where one runs, to start floti `version`
    /// in its place, and says how that went. Nothing it answers stops the
    /// update: floti is already replaced.
    fn relaunch_host(&mut self, version: &Version) {
        let Some(relaunch) = self.relaunch_host else {
            return;
        };
        match relaunch() {
            HostRelaunch::NoHost => {}
            HostRelaunch::Relaunched => self.say(&format!(
                "This computer's host is starting floti {version} in its place; its sessions keep running."
            )),
            HostRelaunch::Failed(why) => self.say(&format!(
                "This computer's host was not restarted ({why}); it runs floti {} until it next starts.",
                self.running_version
            )),
        }
    }

    /// The manifest, its signature checked.
    fn manifest(&self) -> Result<Vec<u8>, UpdateError> {
        let bytes = get_whole(self.network, &self.manifest_url, MANIFEST_LIMIT)
            .map_err(|err| fetch_refusal(MANIFEST_FILE, &self.manifest_url, err))?;
        let signature = fetch_signature(self.network, MANIFEST_FILE, &self.manifest_url)?;
        check_whole(
            self.key,
            &bytes,
            &signature,
            MANIFEST_FILE,
            MANIFEST_COMMENT,
        )?;
        Ok(bytes)
    }

    /// Where the new floti is written: beside the running one, under a
    /// name of this run's own, with the running one's extension.
    fn beside_running(&self, version: &Version) -> PathBuf {
        self.running_binary.with_file_name(format!(
            ".floti-{version}.{}{}",
            std::process::id(),
            self.extension()
        ))
    }

    fn download_all(
        &mut self,
        release: &Release,
        floti: Option<(&FileEntry, &Path)>,
        desktop: Option<&FileEntry>,
        staging: &Path,
    ) -> Result<(), UpdateError> {
        if let Some((entry, path)) = floti {
            self.say(&format!(
                "Downloading floti {} from {}.",
                release.version(),
                release.channel
            ));
            release.folder.fetch(self.network, self.key, entry, path)?;
        }
        if let Some(entry) = desktop {
            std::fs::create_dir_all(staging).map_err(|err| UpdateError::Disk {
                reason: format!("could not make {}: {err}", staging.display()),
            })?;
            self.say(&format!("Downloading {}.", entry.file));
            release
                .folder
                .fetch(self.network, self.key, entry, &staging.join(&entry.file))?;
        }
        Ok(())
    }

    /// Waits until `app` has quit, then installs `downloaded` over it.
    /// `waited` is set once it has found the app running and waited for it,
    /// to the display and session-bus variables the app ran with.
    fn install_desktop(
        &mut self,
        app: &Desktop,
        downloaded: &Path,
        version: &Version,
        floti_updated: bool,
        waited: &mut Option<Vec<(String, String)>>,
    ) -> Result<(), UpdateError> {
        let failed = |reason: String| {
            let (version, app) = (version.to_string(), app.name.to_owned());
            if floti_updated {
                UpdateError::Desktop {
                    version,
                    app,
                    reason,
                }
            } else {
                UpdateError::DesktopAlone {
                    version,
                    app,
                    reason,
                }
            }
        };
        let mut said = false;
        loop {
            let rows = self.processes.processes().map_err(|err| {
                failed(format!(
                    "floti could not tell whether it is running ({err})"
                ))
            })?;
            let Some(running) = rows
                .iter()
                .find(|row| self.installs.is_desktop_process(app, row))
            else {
                break;
            };
            if !said {
                self.report(
                    &format!(
                        "Waiting for {} to quit; it is updated once it has.",
                        app.name
                    ),
                    true,
                );
                said = true;
                *waited = Some(self.installs.display_env(running.pid));
            }
            (self.pause)(APP_POLL);
        }
        self.installs
            .install_desktop(app, downloaded)
            .map_err(|err| failed(err.to_string()))?;
        self.say(&format!("{} is updated to {version}.", app.name));
        if let Some(carried) = self
            .installs
            .carried_floti(app)
            .filter(|carried| carried.is_file())
        {
            match (self.install_self)(&carried) {
                Ok(printed) => {
                    for line in printed.lines().filter(|line| !line.trim().is_empty()) {
                        self.say(line.trim());
                    }
                }
                // The app is updated; its own start puts its floti in place.
                Err(line) => self.say(&format!(
                    "{} could not put its floti and Node in place now ({line}); it does when it next starts.",
                    app.name
                )),
            }
        }
        Ok(())
    }
}

/// The process id in `name` when it is a download an update leaves beside
/// the running binary: `.floti-<version>.<pid><extension>`. Copies put
/// aside are [`Standalone::remove_asides`]'s.
fn leftover_pid(name: &str, extension: &str) -> Option<u32> {
    let base = name.strip_suffix(extension)?;
    let (version, pid) = base.strip_prefix(".floti-")?.rsplit_once('.')?;
    Version::parse(version)?;
    pid.parse().ok()
}

/// Removes the staging folder and everything in it, never walking into a
/// link.
fn clear(staging: &Path) -> Result<(), String> {
    remove_tree(&links::Native, staging).map_err(|err| err.to_string())
}

/// What follows `update`: `--channel <name>` and `--from-desktop`, each at
/// most once, in either order, or nothing.
pub fn parse(args: &[String]) -> Option<Options> {
    let mut options = Options::default();
    let mut rest = args;
    loop {
        match rest {
            [] => return Some(options),
            [flag, name, tail @ ..]
                if flag == "--channel"
                    && options.channel.is_none()
                    && channel::valid_name(name) =>
            {
                options.channel = Some(name.clone());
                rest = tail;
            }
            [flag, tail @ ..] if flag == "--from-desktop" && !options.from_desktop => {
                options.from_desktop = true;
                rest = tail;
            }
            _ => return None,
        }
    }
}

/// `floti update`: the exit status. `relaunch_host` asks this computer's
/// host to start the new floti in its place. A line that cannot be written,
/// to a terminal gone or an output nobody reads, is let go.
pub fn run(options: Options, relaunch_host: RelaunchHost<'_>) -> u8 {
    match run_native(options, relaunch_host) {
        Ok(_) => 0,
        Err(line) => {
            let _ = writeln!(std::io::stderr(), "floti update: {line}");
            1
        }
    }
}

/// Opens the installed `app` as this OS does, alone, with the variables
/// `display` over this process's: it outlives this process and whatever
/// started it.
fn open_native(app: &Desktop, display: &[(String, String)]) -> Result<(), String> {
    let (program, args) = installs::Native.opener(app);
    let failed = |err: std::io::Error| format!("could not run {}: {err}", program.display());
    if display.is_empty() {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        return process_tree::Native
            .start_alone(&program, &args)
            .map(|_| ())
            .map_err(failed);
    }
    // Only where the app's display variables were read: the AppImage.
    let mut command = std::process::Command::new(&program);
    command
        .args(&args)
        .envs(display.iter().map(|(name, value)| (name, value)))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    process_tree::Native.prepare_lasting_root(&mut command);
    command.spawn().map(|_| ()).map_err(failed)
}

fn run_native(options: Options, relaunch_host: RelaunchHost<'_>) -> Result<Outcome, String> {
    let Options {
        channel: chosen_channel,
        from_desktop,
    } = options;
    let env = UserEnv::from_process();
    let folders = folders::resolve(&env).map_err(|err| nothing_updated(&err.to_string()))?;
    let running_binary = running_binary().map_err(|line| nothing_updated(&line))?;
    let network = network::Https::new().map_err(|line| nothing_updated(&line))?;
    let manifest_url = manifest_url(&folders.data);
    let key = signature::trusted_key(&folders.data);
    let pause = |wait: Duration| std::thread::sleep(wait);
    let is_alive = |pid: u32| process_tree::Native.is_alive(pid);
    let mut stdout = std::io::stdout();
    let mut updater = Updater {
        running_version: FLOTI_VERSION,
        target: format!("{}-{}", os_name(), cpu_name()),
        manifest_url,
        key: &key,
        network: &network,
        installs: &installs::Native,
        standalone: &standalone::Native,
        install_self: &run_install_self,
        processes: &process_table::Native,
        is_alive: &is_alive,
        relaunch_host: Some(relaunch_host),
        with_desktop: true,
        pause: &pause,
        env,
        running_binary,
        data: folders.data,
        chosen_channel,
        from_desktop: from_desktop.then_some(FromDesktop { open: &open_native }),
        out: &mut stdout,
        held: None,
    };
    updater.run().map_err(|err| failure_line(&err))
}

/// Runs `<floti> install-self`, from a desktop app just installed, and
/// answers what it printed, or the last line it wrote when it failed.
fn run_install_self(floti: &Path) -> Result<String, String> {
    let output = std::process::Command::new(floti)
        .arg("install-self")
        .output()
        .map_err(|err| format!("could not run {} install-self: {err}", floti.display()))?;
    let printed = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        return Ok(printed);
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let last = |text: &str| -> Option<String> {
        text.lines()
            .rfind(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_owned())
    };
    Err(last(&said)
        .or_else(|| last(&printed))
        .unwrap_or_else(|| "install-self failed and wrote nothing".to_owned()))
}

/// The manifest's address: the built-in one, or, in a build with the
/// testkit only, the one the marker in the data folder `data` names.
pub fn manifest_url(data: &Path) -> Url {
    #[cfg(any(test, feature = "testkit"))]
    if let Some(url) =
        signature::test_marker(data, "manifest").and_then(|text| Url::parse(&text).ok())
    {
        return url;
    }
    let _ = data;
    Url::parse(MANIFEST_URL).expect("the manifest's address is a URL")
}

/// The floti running now, a link it was started through followed to the
/// file it names.
fn running_binary() -> Result<PathBuf, String> {
    let path =
        std::env::current_exe().map_err(|err| format!("floti cannot find its own file: {err}"))?;
    let is_link = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink());
    if is_link {
        std::fs::canonicalize(&path).map_err(|err| format!("floti cannot find its own file: {err}"))
    } else {
        Ok(path)
    }
}
