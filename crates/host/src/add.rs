//! `floti add <client>`: installs a client on this computer and sets it up.
//! The one client it adds is the desktop app (`floti add desktop`), and this
//! is the one way the binary installs it; `floti update` then keeps it up to
//! date. `floti add` with no name, or a name it does not know, lists what it
//! can add.
//!
//! `floti add desktop`:
//!
//! 1. Where the app is already installed ([`Installs::desktop`]) it says
//!    where, and its version where that can be told, and changes nothing.
//! 2. In a session with no display ([`installs::missing_display`]) it
//!    refuses in one sentence saying why.
//! 3. It downloads `update.json` and checks its signature, as `floti update`
//!    does, and finds the channel whose release is floti's own version: the
//!    remembered channel first, then the others. The release's desktop files
//!    are listed with their SHA-256s there alone (the release's
//!    `floti-<version>-SHA256SUMS` lists the floti files, and the app
//!    carries that list, so it cannot list the app). A floti older than a
//!    channel's newest is told to run `floti update` first; one that no
//!    channel names and none is newer than is a development build, with no
//!    published release, and installs nothing.
//! 4. It downloads the release's file for this computer into `add` in the
//!    data folder, checked against its signature and its SHA-256
//!    ([`ReleaseFolder::fetch`]) before anything runs.
//! 5. It installs it where the desktop's installer puts it, per user and with
//!    no admin rights ([`Installs::install_desktop`]): on Windows by running
//!    the release's installer silently, which makes the Start menu entry; on
//!    macOS into `~/Applications`, unpacked with no quarantine flag; on
//!    Linux the AppImage into `~/Applications`, made executable, with a menu
//!    entry and icon ([`Installs::add_launcher`]).
//! 6. Where the OS's installer does not, it runs `floti install-self` from
//!    the floti the new app carries ([`Installs::carried_floti`]), as
//!    `floti update` does, so the Node beside floti is the app's. The app's
//!    floti is this release's, so install-self leaves floti installed on its
//!    own as it is, and the app, which runs install-self at start only when
//!    the floti there is missing or older, uses it and the host already
//!    running: the computer keeps one floti.
//!
//! Progress goes to standard output, one line a step. A failure is one line
//! on standard error starting `floti add desktop: `, saying what failed and
//! that nothing was installed, and the status is 1; the status is 0 when the
//! app was installed or was already there.
//!
//! [`ReleaseFolder::fetch`]: crate::update::release_file::ReleaseFolder::fetch

#[cfg(test)]
mod tests;

use std::io::Write;
use std::path::{Path, PathBuf};

use minisign_verify::PublicKey;
use url::Url;

use crate::bridge::BridgeExit;
use crate::platform::folders::{self, UserEnv};
use crate::platform::installs::{self, Desktop, Installs};
use crate::platform::links::{self, remove_tree};
use crate::update::manifest::{FileEntry, Manifest, Release};
use crate::update::network::{self, Network, get_whole};
use crate::update::release_file::{fetch_refusal, fetch_signature};
use crate::update::signature::{self, MANIFEST_COMMENT, check_whole};
use crate::update::versions::Version;
use crate::update::{UpdateError, channel, manifest_url, run_install_self};
use crate::version::{FLOTI_VERSION, cpu_name, os_name};

/// The clients `floti add` installs, each with what it is.
pub const CLIENTS: &[(&str, &str)] = &[("desktop", "the desktop app, at this floti's own version")];

/// The folder in the data folder a download is kept in until it is
/// installed.
pub const STAGING_FOLDER: &str = "add";

/// The manifest's file name, as its signature and errors name it.
const MANIFEST_FILE: &str = "update.json";

const MANIFEST_LIMIT: u64 = 1024 * 1024;

/// What `floti add` was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// No name: list what it can add.
    List,
    /// `floti add desktop`.
    Desktop,
    /// A name it does not know.
    Unknown(String),
}

/// What follows `add`: nothing, or one name.
pub fn parse(args: &[String]) -> Option<Request> {
    match args {
        [] => Some(Request::List),
        [name] if name == "desktop" => Some(Request::Desktop),
        [name] if !name.starts_with('-') => Some(Request::Unknown(name.clone())),
        _ => None,
    }
}

/// The list of what `floti add` installs, one line each after its first.
pub fn list() -> String {
    let mut text = String::from("floti add <name> installs one of these on this computer:\n");
    let width = CLIENTS
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    for (name, what) in CLIENTS {
        text.push_str(&format!("  {name:width$}  {what}\n"));
    }
    text
}

/// What went wrong, as a fact; [`failure_line`] adds what it means.
#[derive(Debug, thiserror::Error)]
pub enum AddError {
    #[error("{reason}, so {app} is not installed here.")]
    NoDisplay { app: &'static str, reason: String },
    #[error(
        "floti cannot tell where the desktop app goes: no HOME or LOCALAPPDATA names a folder for it."
    )]
    NoTarget,
    #[error(
        "floti {version} has no published release, so there is no {app} for it (this is a development build)."
    )]
    Unreleased { version: String, app: &'static str },
    #[error(
        "floti {version} is older than {channel}'s newest, {newest}, and update.json lists the files of each channel's newest alone; run floti update, then floti add desktop."
    )]
    NotNewest {
        version: String,
        channel: String,
        newest: String,
    },
    #[error("floti {version} has no {app} for {target}.")]
    NoBuild {
        version: String,
        app: &'static str,
        target: String,
    },
    #[error("{app} could not be installed: {reason}")]
    Install { app: &'static str, reason: String },
    /// The app is installed; its menu entry is not.
    #[error("{app} is installed at {path}, but its menu entry could not be written: {reason}")]
    Launcher {
        app: &'static str,
        path: String,
        reason: String,
    },
    #[error(transparent)]
    Update(#[from] UpdateError),
}

/// The line `floti add desktop` prints for `err`: the fact, then that
/// nothing was installed, unless the app was.
pub fn failure_line(err: &AddError) -> String {
    let line = err.to_string();
    match err {
        AddError::Launcher { .. } => line,
        _ => nothing_installed(&line),
    }
}

/// `line` as a sentence, then "Nothing was installed."
fn nothing_installed(line: &str) -> String {
    let stop = if line.ends_with('.') { "" } else { "." };
    format!("{line}{stop} Nothing was installed.")
}

/// How `floti add desktop` ended well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    /// The app was there already; nothing was changed.
    AlreadyInstalled(Desktop),
    /// The app was installed at `version`.
    Installed { desktop: Desktop, version: String },
}

/// What one `floti add desktop` runs with: every piece of the machine it
/// touches, so a test can hand it fakes.
pub struct Adder<'a> {
    /// The version running now, which the app is installed at.
    pub running_version: &'a str,
    /// This computer, as release files name it: `windows-x64`.
    pub target: String,
    pub manifest_url: Url,
    pub key: &'a PublicKey,
    pub network: &'a dyn Network,
    pub installs: &'a dyn Installs,
    /// Runs `<floti> install-self` and answers what it printed, or its
    /// failure line.
    pub install_self: &'a dyn Fn(&Path) -> Result<String, String>,
    /// The user, for where the app goes and whether there is a display.
    pub env: UserEnv,
    /// Floti's data folder: the remembered channel, and the staging folder.
    pub data: PathBuf,
    /// Where progress lines go.
    pub out: &'a mut dyn Write,
}

impl Adder<'_> {
    /// Installs the desktop app, or says why not and changes nothing.
    pub fn run(&mut self) -> Result<Added, AddError> {
        if let Some(app) = self.installs.desktop(&self.env) {
            self.say_installed(&app);
            return Ok(Added::AlreadyInstalled(app));
        }
        let target = self
            .installs
            .desktop_target(&self.env)
            .ok_or(AddError::NoTarget)?;
        if let Some(reason) = installs::missing_display(self.installs, &self.env) {
            return Err(AddError::NoDisplay {
                app: target.name,
                reason: capitalised(&reason),
            });
        }
        let running =
            Version::parse(self.running_version).ok_or_else(|| UpdateError::RunningVersion {
                version: self.running_version.to_owned(),
            })?;
        let release = self.release(&running, target.name)?;
        let entry = release.desktop.clone().ok_or_else(|| AddError::NoBuild {
            version: running.to_string(),
            app: target.name,
            target: self.target.clone(),
        })?;

        let staging = self.data.join(STAGING_FOLDER);
        let installed = self.download_and_install(&release, &entry, &target, &staging);
        let _ = clear(&staging);
        installed?;
        self.say(&format!(
            "{} {running} is installed at {}.",
            target.name,
            target.path.display()
        ));
        match self.installs.add_launcher(&self.env, &target) {
            Ok(Some(entry)) => self.say(&format!("Its menu entry is {}.", entry.display())),
            Ok(None) => {}
            Err(err) => {
                return Err(AddError::Launcher {
                    app: target.name,
                    path: target.path.display().to_string(),
                    reason: err.to_string(),
                });
            }
        }
        self.carried_install_self(&target);
        Ok(Added::Installed {
            desktop: target,
            version: running.to_string(),
        })
    }

    fn say(&mut self, line: &str) {
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
    }

    fn say_installed(&mut self, app: &Desktop) {
        let named = match self.installs.desktop_version(app) {
            Some(version) => format!("{} {version}", app.name),
            None => app.name.to_owned(),
        };
        self.say(&format!(
            "{named} is already installed at {}; floti update keeps it up to date. Nothing was changed.",
            app.path.display()
        ));
    }

    /// The release that is `running`'s, from the signed manifest.
    fn release(&self, running: &Version, app: &'static str) -> Result<Release, AddError> {
        let manifest = Manifest::read(&self.manifest()?)?;
        let versions = manifest.versions();
        let remembered = channel::remembered(&self.data);
        let mut channels: Vec<&(String, Version)> = versions.iter().collect();
        // The remembered channel first; the rest in their names' order.
        channels.sort_by_key(|(name, _)| *name != remembered);
        if let Some((name, _)) = channels.iter().find(|(_, version)| version == running) {
            return Ok(manifest.release(name, &self.target)?);
        }
        match channels.iter().find(|(_, version)| version > running) {
            Some((name, newest)) => Err(AddError::NotNewest {
                version: running.to_string(),
                channel: name.clone(),
                newest: newest.to_string(),
            }),
            None => Err(AddError::Unreleased {
                version: running.to_string(),
                app,
            }),
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

    /// Downloads `entry` into `staging`, checked, and installs it at
    /// `target`.
    fn download_and_install(
        &mut self,
        release: &Release,
        entry: &FileEntry,
        target: &Desktop,
        staging: &Path,
    ) -> Result<(), AddError> {
        clear(staging).map_err(|reason| UpdateError::Disk { reason })?;
        std::fs::create_dir_all(staging).map_err(|err| UpdateError::Disk {
            reason: format!("could not make {}: {err}", staging.display()),
        })?;
        self.say(&format!("Downloading {}.", entry.file));
        let downloaded = staging.join(&entry.file);
        release
            .folder
            .fetch(self.network, self.key, entry, &downloaded)?;
        self.say(&format!(
            "Installing {} in {}.",
            target.name,
            target.path.display()
        ));
        self.installs
            .install_desktop(target, &downloaded)
            .map_err(|err| AddError::Install {
                app: target.name,
                reason: err.to_string(),
            })
    }

    /// Runs `install-self` from the floti the new app carries, where the
    /// OS's installer does not, and says what it printed. A failure is said
    /// and stops nothing: the app runs install-self itself when it starts.
    fn carried_install_self(&mut self, app: &Desktop) {
        let Some(carried) = self
            .installs
            .carried_floti(app)
            .filter(|carried| carried.is_file())
        else {
            return;
        };
        match (self.install_self)(&carried) {
            Ok(printed) => {
                for line in printed.lines().filter(|line| !line.trim().is_empty()) {
                    self.say(line.trim());
                }
            }
            Err(line) => self.say(&format!(
                "{} could not put its floti and Node in place now ({line}); it does when it next starts.",
                app.name
            )),
        }
    }
}

/// `clause` with its first letter capitalised, as a sentence starts.
fn capitalised(clause: &str) -> String {
    let mut chars = clause.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Removes the staging folder and everything in it, never walking into a
/// link.
fn clear(staging: &Path) -> Result<(), String> {
    remove_tree(&links::Native, staging).map_err(|err| err.to_string())
}

/// `floti add` with what followed it: the exit status.
pub fn run(args: &[String]) -> u8 {
    match parse(args) {
        Some(Request::List) => {
            print!("{}", list());
            0
        }
        Some(Request::Unknown(name)) => {
            eprint!("floti add: floti cannot add \"{name}\".\n{}", list());
            BridgeExit::Usage.code()
        }
        Some(Request::Desktop) => match run_native() {
            Ok(_) => 0,
            Err(line) => {
                let _ = writeln!(std::io::stderr(), "floti add desktop: {line}");
                1
            }
        },
        None => {
            eprint!("usage: floti add [<name>]\n{}", list());
            BridgeExit::Usage.code()
        }
    }
}

fn run_native() -> Result<Added, String> {
    let env = UserEnv::from_process();
    let folders = folders::resolve(&env).map_err(|err| nothing_installed(&err.to_string()))?;
    let network = network::Https::new().map_err(|line| nothing_installed(&line))?;
    let key = signature::trusted_key(&folders.data);
    let mut stdout = std::io::stdout();
    let mut adder = Adder {
        running_version: FLOTI_VERSION,
        target: format!("{}-{}", os_name(), cpu_name()),
        manifest_url: manifest_url(&folders.data),
        key: &key,
        network: &network,
        installs: &installs::Native,
        install_self: &run_install_self,
        env,
        data: folders.data,
        out: &mut stdout,
    };
    adder.run().map_err(|err| failure_line(&err))
}
