//! `floti add <client>`: installs a client on this computer and sets it up.
//! The one client it adds is the desktop app (`floti add desktop`), and this
//! is the one way the binary installs it; `floti update` then keeps it up to
//! date. `floti add` with no name, or a name it does not know, lists what it
//! can add.
//!
//! `floti add desktop`:
//!
//! 0. It holds `floti update`'s run file throughout ([`RunLock`]), so a
//!    second `floti add desktop`, or one while `floti update` runs, refuses
//!    in one line, and two never write the same download. It writes no
//!    ended record: the host follows updates by it, and an add is none.
//! 1. Where the app is already installed ([`Installs::desktop`]) it says
//!    where, and its version where that can be told, and changes nothing
//!    but a launcher entry floti makes that is missing
//!    ([`Installs::repair_launcher`]). Where something else is at the
//!    install's place (a folder with no app in it, an interrupted copy) it
//!    refuses in one line and leaves it as it is.
//! 2. In a session with no display ([`installs::missing_display`]) it
//!    refuses in one sentence saying why.
//! 3. It reads `Helm-<version>-SHA256SUMS` from the release that is floti's
//!    own version, preview or stable, under the project's GitHub releases
//!    ([`release_file::releases_url`]): its signature first, made with the
//!    key built into floti and the trusted comment `floti <version> <file>`,
//!    then the list. The app carries floti's own list
//!    (`floti-<version>-SHA256SUMS`), so that list cannot hold the app; this
//!    one is the desktop files'. A version with no published release (a
//!    development build), or a release without the list, says so and
//!    installs nothing.
//! 4. It downloads the release's file for this computer
//!    ([`desktop_file`]) into `add` in the data folder, checked against its
//!    own signature and its SHA-256 in the list ([`ReleaseFolder::fetch`])
//!    before anything runs.
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
//! [`RunLock`]: crate::update::run_file::RunLock
//! [`release_file::releases_url`]: crate::update::release_file::releases_url

#[cfg(test)]
mod tests;

use std::io::Write;
use std::path::{Path, PathBuf};

use floti_protocol::Knowable;
use minisign_verify::PublicKey;
use url::Url;

use crate::bridge::BridgeExit;
use crate::platform::folders::{self, UserEnv};
use crate::platform::installs::{self, Desktop, Installs};
use crate::platform::links::{self, remove_tree};
use crate::platform::process_tree::{self, ProcessTree};
use crate::update::manifest::FileEntry;
use crate::update::network::{self, FetchError, Network, get_whole};
use crate::update::release_file::{self, ReleaseFolder, signature_url};
use crate::update::run_file::RunLock;
use crate::update::signature;
use crate::update::versions::Version;
use crate::update::{UpdateError, run_install_self};
use crate::version::{FLOTI_VERSION, cpu_name, os_name};

/// The clients `floti add` installs, each with what it is.
pub const CLIENTS: &[(&str, &str)] = &[("desktop", "the desktop app, at this floti's own version")];

/// The folder in the data folder a download is kept in until it is
/// installed.
pub const STAGING_FOLDER: &str = "add";

/// What `floti add desktop` says in the run file while it holds it.
pub const STARTING_LINE: &str = "Starting floti add desktop.";

/// The largest SHA-256 list fetched.
const SUMS_LIMIT: u64 = 64 * 1024;

/// The largest signature fetched, when looking whether a release is there.
const SIGNATURE_LIMIT: u64 = 16 * 1024;

/// The desktop file a release of `version` has for `target`, as the release
/// workflow names it (`desktopFileNames` in
/// `packages/desktop/scripts/release.mjs`): `None` for a target with no
/// desktop app.
pub fn desktop_file(version: &Version, target: &str) -> Option<String> {
    match target {
        "windows-x64" => Some(format!("Helm-{version}-setup.exe")),
        "macos-arm64" => Some(format!("Helm-{version}-arm64-mac.zip")),
        "linux-x64" => Some(format!("Helm-{version}-linux-x64.AppImage")),
        _ => None,
    }
}

/// The signed SHA-256 list of a release's desktop files
/// (`desktopSumsName` in `packages/desktop/scripts/release.mjs`).
pub fn desktop_sums_name(version: &Version) -> String {
    format!("Helm-{version}-SHA256SUMS")
}

/// The SHA-256 `list`, in sha256sum's format (`<hex>  <name>` or
/// `<hex> *<name>`), gives `file`, when it gives one.
pub fn listed_sha256(list: &str, file: &str) -> Option<String> {
    list.lines().find_map(|line| {
        let line = line.trim_end_matches('\r');
        let (sha256, rest) = line.split_at_checked(64)?;
        let name = rest.strip_prefix(' ')?;
        let name = name.strip_prefix(['*', ' '])?;
        (name == file && sha256.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| sha256.to_ascii_lowercase())
    })
}

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
        "{path} is there but is not an install of {app}, so nothing was installed; move it away to install {app} there."
    )]
    NotAnInstall { path: String, app: &'static str },
    #[error(
        "another floti update or floti add desktop is running (process {pid}), so nothing was installed; run floti add desktop again once it has ended."
    )]
    Busy { pid: u32 },
    #[error(
        "floti {version} has no published release, so there is no {app} for it (this is a development build)."
    )]
    Unreleased { version: String, app: &'static str },
    #[error(
        "floti {version}'s release has no {list}, the signed list of its desktop files' SHA-256s, so floti cannot check {app} for it."
    )]
    NoList {
        version: String,
        list: String,
        app: &'static str,
    },
    #[error("{list} does not list {file}.")]
    NotListed { list: String, file: String },
    #[error("{file} does not match its SHA-256 in {list}.")]
    WrongSha256 { file: String, list: String },
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
        // Each says itself what was and was not installed.
        AddError::Launcher { .. }
        | AddError::NoDisplay { .. }
        | AddError::NotAnInstall { .. }
        | AddError::Busy { .. } => line,
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
    /// Where every release's folder is ([`release_file::releases_url`]).
    pub releases: Url,
    pub key: &'a PublicKey,
    pub network: &'a dyn Network,
    pub installs: &'a dyn Installs,
    /// Runs `<floti> install-self` and answers what it printed, or its
    /// failure line.
    pub install_self: &'a dyn Fn(&Path) -> Result<String, String>,
    /// Whether a process is alive, for a run file another left.
    pub is_alive: &'a dyn Fn(u32) -> Knowable<bool>,
    /// The user, for where the app goes and whether there is a display.
    pub env: UserEnv,
    /// Floti's data folder, for the staging folder.
    pub data: PathBuf,
    /// Where progress lines go.
    pub out: &'a mut dyn Write,
}

impl Adder<'_> {
    /// Installs the desktop app, or says why not and changes nothing,
    /// holding `floti update`'s run file throughout. Dropped, the run file
    /// is removed; no ended record is written.
    pub fn run(&mut self) -> Result<Added, AddError> {
        let _held = RunLock::take(&self.data, std::process::id(), STARTING_LINE, self.is_alive)
            .map_err(|err| match err {
                UpdateError::AlreadyRunning { pid } => AddError::Busy { pid },
                other => other.into(),
            })?;
        self.run_held()
    }

    fn run_held(&mut self) -> Result<Added, AddError> {
        if let Some(app) = self.installs.desktop(&self.env) {
            // One line, after the repair, so it says whether anything changed;
            // a repair that fails is its own line, which says the app is there.
            return match self.installs.repair_launcher(&self.env, &app) {
                Ok(repaired) => {
                    let line = match repaired {
                        Some(entry) => repaired_line(self.installs, &app, &entry),
                        None => already_installed_line(self.installs, &app),
                    };
                    self.say(&line);
                    Ok(Added::AlreadyInstalled(app))
                }
                Err(err) => Err(AddError::Launcher {
                    app: app.name,
                    path: app.path.display().to_string(),
                    reason: err.to_string(),
                }),
            };
        }
        let target = self
            .installs
            .desktop_target(&self.env)
            .ok_or(AddError::NoTarget)?;
        if std::fs::symlink_metadata(&target.path).is_ok() {
            return Err(AddError::NotAnInstall {
                path: target.path.display().to_string(),
                app: target.name,
            });
        }
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
        let file = desktop_file(&running, &self.target).ok_or_else(|| AddError::NoBuild {
            version: running.to_string(),
            app: target.name,
            target: self.target.clone(),
        })?;
        let releases = self.releases.to_string();
        let folder =
            ReleaseFolder::of_version(&self.releases, running.clone()).ok_or_else(|| {
                UpdateError::Network {
                    file: file.clone(),
                    url: releases,
                    reason: "it is not an https folder".into(),
                }
            })?;
        let entry = self.listed_entry(&folder, &file, target.name)?;

        let staging = self.data.join(STAGING_FOLDER);
        let installed = self.download_and_install(&folder, &entry, &target, &staging);
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

    /// `file`'s entry in the release's signed desktop list: the list's
    /// signature checked first, then the list.
    fn listed_entry(
        &self,
        folder: &ReleaseFolder,
        file: &str,
        app: &'static str,
    ) -> Result<FileEntry, AddError> {
        let version = folder.version().to_string();
        let list = desktop_sums_name(folder.version());
        let bytes = match folder.fetch_whole(self.network, self.key, &list, SUMS_LIMIT) {
            Ok(bytes) => bytes,
            Err(UpdateError::MissingSignature { .. }) => {
                return Err(if self.published(folder, file)? {
                    AddError::NoList { version, list, app }
                } else {
                    AddError::Unreleased { version, app }
                });
            }
            Err(err) => return Err(err.into()),
        };
        let sha256 = listed_sha256(&String::from_utf8_lossy(&bytes), file).ok_or_else(|| {
            AddError::NotListed {
                list: list.clone(),
                file: file.to_owned(),
            }
        })?;
        Ok(FileEntry::checked(file, &sha256)?)
    }

    /// Whether the release in `folder` is published: whether its `file`'s
    /// signature is there.
    fn published(&self, folder: &ReleaseFolder, file: &str) -> Result<bool, UpdateError> {
        let url = signature_url(&folder.file_url(file));
        match get_whole(self.network, &url, SIGNATURE_LIMIT) {
            Ok(_) => Ok(true),
            Err(FetchError::NotFound) => Ok(false),
            Err(err) => Err(release_file::fetch_refusal(
                &format!("{file}.minisig"),
                &url,
                err,
            )),
        }
    }

    /// Downloads `entry` into `staging`, checked, and installs it at
    /// `target`.
    fn download_and_install(
        &mut self,
        folder: &ReleaseFolder,
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
        folder
            .fetch(self.network, self.key, entry, &downloaded)
            .map_err(|err| match err {
                UpdateError::WrongSha256 { file } => AddError::WrongSha256 {
                    file,
                    list: desktop_sums_name(folder.version()),
                },
                other => other.into(),
            })?;
        self.say(&format!(
            "Installing {} in {}.",
            target.name,
            target.path.display()
        ));
        self.installs
            .install_desktop(target, &downloaded)
            .map(|_| ())
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

/// What `floti add desktop`, and `floti setup`, say of the installed `app`
/// when nothing was changed: where it is, and its version where `installs`
/// can tell it.
pub fn already_installed_line(installs: &dyn Installs, app: &Desktop) -> String {
    format!("{} Nothing was changed.", installed_clause(installs, app))
}

/// What `floti add desktop` says of the installed `app` once it wrote the
/// menu entry that was missing, at `entry`.
pub fn repaired_line(installs: &dyn Installs, app: &Desktop, entry: &Path) -> String {
    format!(
        "{} Its menu entry was missing, so floti added it at {}.",
        installed_clause(installs, app),
        entry.display()
    )
}

/// Where the installed `app` is, and its version where `installs` can tell
/// it, as the sentence both lines start with.
fn installed_clause(installs: &dyn Installs, app: &Desktop) -> String {
    let named = match installs.desktop_version(app) {
        Some(version) => format!("{} {version}", app.name),
        None => app.name.to_owned(),
    };
    format!(
        "{named} is already installed at {}; floti update keeps it up to date.",
        app.path.display()
    )
}

/// `clause` with its first letter capitalised, as a sentence starts.
pub(crate) fn capitalised(clause: &str) -> String {
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
    add_desktop_here(&mut std::io::stdout())
}

/// `floti add desktop` for this process's user, on this computer, its
/// progress on `out`: how it ended well, or its failure line
/// ([`failure_line`]). `floti setup` installs the app through it.
pub fn add_desktop_here(out: &mut dyn Write) -> Result<Added, String> {
    let env = UserEnv::from_process();
    let folders = folders::resolve(&env).map_err(|err| nothing_installed(&err.to_string()))?;
    add_desktop(env, folders.data, &installs::Native, out)
}

/// `floti add desktop` for the user `env` describes, with floti's data folder
/// `data`, installing through `installs`: this release's app, from the
/// release folder and key the data folder gives ([`release_file::releases_url`],
/// [`signature::trusted_key`]), over HTTPS. How it ended well, or its failure
/// line.
pub fn add_desktop(
    env: UserEnv,
    data: PathBuf,
    installs: &dyn Installs,
    out: &mut dyn Write,
) -> Result<Added, String> {
    let network = network::Https::new().map_err(|line| nothing_installed(&line))?;
    let key = signature::trusted_key(&data);
    let is_alive = |pid: u32| process_tree::Native.is_alive(pid);
    let mut adder = Adder {
        running_version: FLOTI_VERSION,
        target: format!("{}-{}", os_name(), cpu_name()),
        releases: release_file::releases_url(&data),
        key: &key,
        network: &network,
        installs,
        install_self: &run_install_self,
        is_alive: &is_alive,
        env,
        data,
        out,
    };
    adder.run().map_err(|err| failure_line(&err))
}
