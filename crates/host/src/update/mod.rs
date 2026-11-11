//! `floti update [--channel <name>]`: replaces the running floti with its
//! channel's newest release, and the desktop app where it is installed and
//! older, once the app has quit.
//!
//! 1. It removes what an earlier update left beside the running floti
//!    (`floti.old-<pid>.exe`, `.floti-<version>.<pid>[.exe]`) where the
//!    process that left it is known to have ended.
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
//!    updated. Where installing the app stops this computer's host
//!    (Windows), and that host runs sessions, it says how many end and asks
//!    on the terminal, no by default; with no terminal to ask on it leaves
//!    the app and says why. With nothing to replace, it says floti is up to
//!    date and changes nothing.
//! 4. It downloads this computer's floti beside the running one, under a
//!    name of its own, and the app's release file into `update` in the data
//!    folder, each checked against its signature and its SHA-256
//!    ([`release_file`]).
//! 5. Only when everything has passed does it replace anything: floti
//!    first ([`Installs::replace_binary`]), then, once no process of the
//!    app is left, the app ([`Installs::install_desktop`]). While the app
//!    runs it says it is waiting for it to quit, and looks again every
//!    second.
//!
//! Any failed check stops the update with one line on standard error that
//! names the file and what failed, and nothing is replaced; what was
//! downloaded is removed. Progress goes to standard output, one line a step.
//! Exit status 0 when floti is up to date or updated, 1 otherwise.
//!
//! What is checked and replaced is the per-OS concern
//! [`platform::installs`](crate::platform::installs). Restarting a running
//! host on the new binary is not part of it.

pub mod channel;
pub mod manifest;
pub mod network;
pub mod release_file;
pub mod signature;
pub mod versions;

#[cfg(test)]
mod tests;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use floti_protocol::Knowable;
use minisign_verify::PublicKey;
use url::Url;

use crate::platform::folders::{self, UserEnv};
use crate::platform::installs::{self, Desktop, Installs};
use crate::platform::links::{self, remove_tree};
use crate::platform::process_table::{self, ProcessTable};
use crate::platform::process_tree::{self, ProcessTree};
use crate::version::{FLOTI_VERSION, cpu_name, os_name};
use manifest::{FileEntry, Release};
use network::{Network, get_whole};
use release_file::{fetch_refusal, fetch_signature};
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
    /// Only the app had an update, and it was left as it is: the person
    /// said no, or there was no terminal to ask on.
    DesktopLeft { version: String, app: String },
}

/// Counts the sessions this computer's host runs, which installing the
/// desktop app over its install ends where that stops the host; 0 when no
/// host runs.
pub type SessionsEnding<'a> = &'a dyn Fn() -> Result<usize, String>;

/// Asks a person a yes-or-no question, no by default; `None` when there is
/// no terminal to ask on.
pub type Confirm<'a> = &'a dyn Fn(&str) -> Option<bool>;

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
    pub processes: &'a dyn ProcessTable,
    /// Whether a process is alive, for what earlier updates left.
    pub is_alive: &'a dyn Fn(u32) -> Knowable<bool>,
    pub sessions_ending: SessionsEnding<'a>,
    pub confirm: Confirm<'a>,
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
    /// Where progress lines go.
    pub out: &'a mut dyn Write,
}

impl Updater<'_> {
    /// Runs the update.
    pub fn run(&mut self) -> Result<Outcome, UpdateError> {
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

        let installed = self.installs.desktop(&self.env);
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
        let mut desktop = self.desktop_step(installed, &release);
        let mut left = None;
        if let Some((app, _)) = &desktop
            && !self.may_install_desktop(app)
        {
            left = Some(app.name.to_owned());
            desktop = None;
        }
        if floti.is_none() && desktop.is_none() {
            if let Some(app) = left {
                return Ok(Outcome::DesktopLeft {
                    version: version.to_string(),
                    app,
                });
            }
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
            if let Err(err) = self.installs.replace_binary(&self.running_binary, new) {
                discard(&new_binary);
                return Err(UpdateError::Replace {
                    reason: err.to_string(),
                });
            }
            self.say(&format!(
                "floti is updated from {} to {version}.",
                self.running_version
            ));
        }

        let updated_desktop = match desktop {
            Some((app, entry)) => {
                let result = self.install_desktop(
                    &app,
                    &staging.join(&entry.file),
                    &version,
                    new_binary.is_some(),
                );
                let _ = clear(&staging);
                result?;
                Some(app.name.to_owned())
            }
            None => {
                let _ = clear(&staging);
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
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
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
            .installs
            .standalone_binary(&self.env)
            .filter(|path| path.is_file() && *path != self.running_binary)
        {
            line.push_str(&format!(
                " floti installed on its own is {0}; `{0} update` updates it.",
                standalone.display()
            ));
        }
        self.say(&line);
    }

    /// Removes what an earlier update left beside the running floti, where
    /// the process that left it has ended. Nothing it cannot remove stops
    /// this update.
    fn sweep_leftovers(&self) {
        let Some(folder) = self.running_binary.parent() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        let stem = self
            .running_binary
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = self.extension();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name
                .to_str()
                .and_then(|name| leftover_pid(name, &stem, &extension))
            else {
                continue;
            };
            if matches!((self.is_alive)(pid), Knowable::Known(false)) {
                let _ = std::fs::remove_file(entry.path());
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

    /// Whether `app` may be installed now. Where that stops this computer's
    /// host and the host runs sessions, the person is asked first.
    fn may_install_desktop(&mut self, app: &Desktop) -> bool {
        if !self.installs.installing_desktop_stops_host() {
            return true;
        }
        let ending = match (self.sessions_ending)() {
            Ok(0) => return true,
            Ok(1) => "1 session; it comes back".to_owned(),
            Ok(running) => format!("{running} sessions; they come back"),
            Err(why) => {
                self.say(&format!(
                    "floti could not ask this computer's host which sessions updating {} ends: {why}",
                    app.name
                ));
                "the sessions it runs; they come back".to_owned()
            }
        };
        let name = app.name;
        let question = format!(
            "Updating {name} stops this computer's host, ending {ending} when {name} next starts. Update {name} now?"
        );
        match (self.confirm)(&question) {
            Some(true) => true,
            Some(false) => {
                self.say(&format!(
                    "{name} stays as it is; floti update updates it when run again."
                ));
                false
            }
            None => {
                self.say(&format!(
                    "{name} stays as it is: updating it ends sessions, and there is no terminal to ask on first. Run floti update in a terminal to update {name}."
                ));
                false
            }
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
    fn install_desktop(
        &mut self,
        app: &Desktop,
        downloaded: &Path,
        version: &Version,
        floti_updated: bool,
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
            if !rows
                .iter()
                .any(|row| self.installs.is_desktop_process(app, row))
            {
                break;
            }
            if !said {
                self.say(&format!(
                    "Waiting for {} to quit; it is updated once it has.",
                    app.name
                ));
                said = true;
            }
            (self.pause)(APP_POLL);
        }
        self.installs
            .install_desktop(app, downloaded)
            .map_err(|err| failed(err.to_string()))?;
        self.say(&format!("{} is updated to {version}.", app.name));
        Ok(())
    }
}

/// The process id in `name` when it is something an update leaves beside
/// the running binary `<stem><extension>`: `<stem>.old-<pid><extension>`,
/// the binary moved aside, or `.floti-<version>.<pid><extension>`, a
/// download.
fn leftover_pid(name: &str, stem: &str, extension: &str) -> Option<u32> {
    let base = name.strip_suffix(extension)?;
    if let Some(pid) = base.strip_prefix(&format!("{stem}.old-")) {
        return pid.parse().ok();
    }
    let (version, pid) = base.strip_prefix(".floti-")?.rsplit_once('.')?;
    Version::parse(version)?;
    pid.parse().ok()
}

/// Removes the staging folder and everything in it, never walking into a
/// link.
fn clear(staging: &Path) -> Result<(), String> {
    remove_tree(&links::Native, staging).map_err(|err| err.to_string())
}

/// What follows `update`: nothing, or `--channel <name>`.
pub fn parse(args: &[String]) -> Option<Option<String>> {
    match args {
        [] => Some(None),
        [flag, name] if flag == "--channel" && channel::valid_name(name) => {
            Some(Some(name.clone()))
        }
        _ => None,
    }
}

/// `floti update`, run by a person: the exit status. `sessions_ending`
/// counts the sessions this computer's host runs.
pub fn run(chosen_channel: Option<String>, sessions_ending: SessionsEnding<'_>) -> u8 {
    match run_native(chosen_channel, sessions_ending) {
        Ok(_) => 0,
        Err(line) => {
            eprintln!("floti update: {line}");
            1
        }
    }
}

fn run_native(
    chosen_channel: Option<String>,
    sessions_ending: SessionsEnding<'_>,
) -> Result<Outcome, String> {
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
        processes: &process_table::Native,
        is_alive: &is_alive,
        sessions_ending,
        confirm: &ask_on_terminal,
        pause: &pause,
        env,
        running_binary,
        data: folders.data,
        chosen_channel,
        out: &mut stdout,
    };
    updater.run().map_err(|err| failure_line(&err))
}

/// Asks `question` on the terminal, no by default; `None` when standard
/// input is not a terminal.
fn ask_on_terminal(question: &str) -> Option<bool> {
    use std::io::{BufRead, IsTerminal};
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return None;
    }
    let mut out = std::io::stdout();
    let _ = write!(out, "{question} [y/N] ");
    let _ = out.flush();
    let mut answer = String::new();
    if stdin.lock().read_line(&mut answer).is_err() {
        return Some(false);
    }
    let answer = answer.trim();
    Some(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
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
