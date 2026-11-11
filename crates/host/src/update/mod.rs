//! `floti update [--channel <name>]`: replaces the running floti with its
//! channel's newest release, then the desktop app where it is installed,
//! once the app has quit.
//!
//! 1. It downloads `https://floti.dev/update.json` and its signature
//!    `update.json.minisig`, and checks the signature with the key built
//!    into the binary and its trusted comment, `floti update.json`
//!    ([`signature`]). The channel is the one `--channel` names, which is
//!    then remembered ([`channel`]), or the one remembered, `stable` at
//!    first. A channel the manifest does not have is an error naming it.
//! 2. When the channel's version is not newer than this floti's, it says
//!    floti is up to date and changes nothing: an update never installs an
//!    older or equal version.
//! 3. It downloads this computer's floti (`<url><file>`) beside the running
//!    one, under a name of its own, and, where the desktop app is
//!    installed, the release's desktop file into `update` in the data
//!    folder. Each file's signature (`<url><file>.minisig`) is fetched
//!    first; once the file is on disk it is checked against that
//!    signature, whose trusted comment must be `floti <version> <file>`,
//!    and against its SHA-256 in the manifest.
//! 4. Only when everything has passed does it replace anything: floti
//!    first ([`Installs::replace_binary`]), then, once no process of the
//!    app is left, the app ([`Installs::replace_desktop`]). While the app
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
pub mod signature;
pub mod versions;

#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use minisign_verify::PublicKey;
use url::Url;

use crate::platform::folders::{self, UserEnv};
use crate::platform::installs::{self, Desktop, Installs};
use crate::platform::links::{self, remove_tree};
use crate::platform::process_table::{self, ProcessTable};
use crate::version::{FLOTI_VERSION, cpu_name, os_name};
use manifest::{FileEntry, Release};
use network::{FetchError, Network, get_whole};
use signature::{FileCheck, MANIFEST_COMMENT, check_whole, file_comment};
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
const SIGNATURE_LIMIT: u64 = 16 * 1024;
const FILE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// Why an update stopped, as the one line it prints.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("could not download {file} from {url}: {reason}, so nothing was updated.")]
    Network {
        file: String,
        url: String,
        reason: String,
    },
    #[error("{file} has no signature ({file}.minisig is not there), so nothing was updated.")]
    MissingSignature { file: String },
    #[error("{file} does not match its signature, so nothing was updated.")]
    BadSignature { file: String },
    #[error("{file} is not signed with the key built into floti, so nothing was updated.")]
    WrongKey { file: String },
    #[error(
        "the signature of {file} is for \"{found}\", not \"{wanted}\", so nothing was updated."
    )]
    WrongComment {
        file: String,
        found: String,
        wanted: String,
    },
    #[error("{file} does not match its SHA-256 in update.json, so nothing was updated.")]
    WrongSha256 { file: String },
    #[error("the download of {file} stopped before its end ({reason}), so nothing was updated.")]
    CutOff { file: String, reason: String },
    #[error("{0}")]
    Manifest(String),
    #[error("update.json has no channel \"{channel}\"; it has {}.", known.join(", "))]
    NoChannel { channel: String, known: Vec<String> },
    #[error("floti {version} on {channel} has no build for {target}, so nothing was updated.")]
    NoBuild {
        channel: String,
        version: String,
        target: String,
    },
    #[error("{reason}, so nothing was updated.")]
    Disk { reason: String },
    #[error("floti could not be replaced, so nothing was updated: {reason}")]
    Replace { reason: String },
    #[error("floti is updated to {version}, but {app} is not: {reason}")]
    Desktop {
        version: String,
        app: String,
        reason: String,
    },
}

/// How an update ended well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The channel has nothing newer than this floti.
    UpToDate { channel: String, newest: String },
    /// floti was replaced, and the desktop app with it when `desktop` names it.
    Updated {
        version: String,
        desktop: Option<String>,
    },
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
    pub processes: &'a dyn ProcessTable,
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
        let channel = self
            .chosen_channel
            .clone()
            .unwrap_or_else(|| channel::remembered(&self.data));
        let manifest = self.manifest()?;
        let release = manifest::release(&manifest, &channel, &self.target)?;
        if self.chosen_channel.is_some() {
            channel::remember(&self.data, &channel)?;
        }
        let running = Version::parse(self.running_version);
        if running.is_some_and(|running| release.version <= running) {
            self.say(&format!(
                "floti {} is up to date on {channel} (its newest is {}).",
                self.running_version, release.version
            ));
            return Ok(Outcome::UpToDate {
                channel,
                newest: release.version.to_string(),
            });
        }
        let floti = release.floti.clone().ok_or_else(|| UpdateError::NoBuild {
            channel: channel.clone(),
            version: release.version.to_string(),
            target: self.target.clone(),
        })?;
        let desktop = self.installs.desktop(&self.env);
        let desktop_file = match (&desktop, &release.desktop) {
            (Some(_), Some(entry)) => Some(entry.clone()),
            (Some(app), None) => {
                self.say(&format!(
                    "floti {} has no {} for {}, so {} stays as it is.",
                    release.version, app.name, self.target, app.name
                ));
                None
            }
            (None, _) => None,
        };

        let staging = self.data.join(STAGING_FOLDER);
        clear(&staging).map_err(|reason| UpdateError::Disk { reason })?;
        let new_binary = self.beside_running(&release.version);
        let checked = self.download_all(
            &release,
            &floti,
            &new_binary,
            desktop_file.as_ref(),
            &staging,
        );
        if let Err(err) = checked {
            let _ = std::fs::remove_file(&new_binary);
            let _ = clear(&staging);
            return Err(err);
        }

        if let Err(err) = self
            .installs
            .replace_binary(&self.running_binary, &new_binary)
        {
            let _ = std::fs::remove_file(&new_binary);
            let _ = clear(&staging);
            return Err(UpdateError::Replace {
                reason: err.to_string(),
            });
        }
        self.say(&format!(
            "floti is updated from {} to {}.",
            self.running_version, release.version
        ));

        let updated_desktop = match (desktop, desktop_file) {
            (Some(app), Some(entry)) => {
                let result = self.replace_desktop(&app, &staging.join(&entry.file), &release);
                let _ = clear(&staging);
                result?;
                Some(app.name.to_owned())
            }
            _ => {
                let _ = clear(&staging);
                None
            }
        };
        Ok(Outcome::Updated {
            version: release.version.to_string(),
            desktop: updated_desktop,
        })
    }

    fn say(&mut self, line: &str) {
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
    }

    /// The manifest, its signature checked.
    fn manifest(&self) -> Result<Vec<u8>, UpdateError> {
        let bytes = get_whole(self.network, &self.manifest_url, MANIFEST_LIMIT)
            .map_err(|err| fetch_refusal(MANIFEST_FILE, &self.manifest_url, err))?;
        let signature_url = signature_url(&self.manifest_url);
        let signature = self.signature(MANIFEST_FILE, &signature_url)?;
        check_whole(
            self.key,
            &bytes,
            &signature,
            MANIFEST_FILE,
            MANIFEST_COMMENT,
        )?;
        Ok(bytes)
    }

    fn signature(&self, file: &str, url: &Url) -> Result<Vec<u8>, UpdateError> {
        get_whole(self.network, url, SIGNATURE_LIMIT).map_err(|err| match err {
            FetchError::NotFound => UpdateError::MissingSignature {
                file: file.to_owned(),
            },
            other => fetch_refusal(&format!("{file}.minisig"), url, other),
        })
    }

    /// Where the new floti is written: beside the running one, under a
    /// name of this run's own, with the running one's extension.
    fn beside_running(&self, version: &Version) -> PathBuf {
        let extension = self
            .running_binary
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        self.running_binary.with_file_name(format!(
            ".floti-{version}.{}{extension}",
            std::process::id()
        ))
    }

    fn download_all(
        &mut self,
        release: &Release,
        floti: &FileEntry,
        new_binary: &Path,
        desktop: Option<&FileEntry>,
        staging: &Path,
    ) -> Result<(), UpdateError> {
        self.say(&format!(
            "Downloading floti {} from {}.",
            release.version, release.channel
        ));
        self.download(release, floti, new_binary)?;
        if let Some(entry) = desktop {
            std::fs::create_dir_all(staging).map_err(|err| UpdateError::Disk {
                reason: format!("could not make {}: {err}", staging.display()),
            })?;
            self.say(&format!("Downloading {}.", entry.file));
            self.download(release, entry, &staging.join(&entry.file))?;
        }
        Ok(())
    }

    /// Downloads `entry` to `path` and checks it. A file that fails a check
    /// is removed.
    fn download(
        &self,
        release: &Release,
        entry: &FileEntry,
        path: &Path,
    ) -> Result<(), UpdateError> {
        let url = release.file_url(&entry.file);
        let signature = self.signature(&entry.file, &signature_url(&url))?;
        let comment = file_comment(&release.version.to_string(), &entry.file);
        let check = FileCheck::new(self.key, &signature, &entry.file, &comment)?;
        let result = (|| {
            let mut file = File::create(path).map_err(|err| UpdateError::Disk {
                reason: format!("could not write {}: {err}", path.display()),
            })?;
            self.network
                .get(&url, FILE_LIMIT, &mut |piece| file.write_all(piece))
                .map_err(|err| fetch_refusal(&entry.file, &url, err))?;
            file.sync_all().map_err(|err| UpdateError::Disk {
                reason: format!("could not write {}: {err}", path.display()),
            })?;
            drop(file);
            check.finish(self.key, path, &entry.sha256)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(path);
        }
        result
    }

    /// Waits until `app` has quit, then replaces it with `downloaded`.
    fn replace_desktop(
        &mut self,
        app: &Desktop,
        downloaded: &Path,
        release: &Release,
    ) -> Result<(), UpdateError> {
        let failed = |reason: String| UpdateError::Desktop {
            version: release.version.to_string(),
            app: app.name.to_owned(),
            reason,
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
            .replace_desktop(app, downloaded)
            .map_err(|err| failed(err.to_string()))?;
        self.say(&format!("{} is updated to {}.", app.name, release.version));
        Ok(())
    }
}

/// `url` with `.minisig` after it.
fn signature_url(url: &Url) -> Url {
    Url::parse(&format!("{url}.minisig")).expect("a URL with .minisig after it is a URL")
}

fn fetch_refusal(file: &str, url: &Url, err: FetchError) -> UpdateError {
    match err {
        FetchError::Cut(reason) => UpdateError::CutOff {
            file: file.to_owned(),
            reason,
        },
        other => UpdateError::Network {
            file: file.to_owned(),
            url: url.to_string(),
            reason: other.to_string(),
        },
    }
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

/// `floti update`, run by a person: the exit status.
pub fn run(chosen_channel: Option<String>) -> u8 {
    match run_native(chosen_channel) {
        Ok(_) => 0,
        Err(line) => {
            eprintln!("floti update: {line}");
            1
        }
    }
}

fn run_native(chosen_channel: Option<String>) -> Result<Outcome, String> {
    let env = UserEnv::from_process();
    let folders = folders::resolve(&env).map_err(|err| err.to_string())?;
    let running_binary = running_binary()?;
    let network = network::Https::new()?;
    let (manifest_url, key) = manifest_and_key(&folders.data);
    let pause = |wait: Duration| std::thread::sleep(wait);
    let mut stdout = std::io::stdout();
    let mut updater = Updater {
        running_version: FLOTI_VERSION,
        target: format!("{}-{}", os_name(), cpu_name()),
        manifest_url,
        key: &key,
        network: &network,
        installs: &installs::Native,
        processes: &process_table::Native,
        pause: &pause,
        env,
        running_binary,
        data: folders.data,
        chosen_channel,
        out: &mut stdout,
    };
    updater.run().map_err(|err| err.to_string())
}

/// The manifest's address and the key it and every file are checked with.
fn manifest_and_key(data: &Path) -> (Url, PublicKey) {
    #[cfg(any(test, feature = "testkit"))]
    if let Some(overrides) = signature::test_overrides(data) {
        return overrides;
    }
    let _ = data;
    (
        Url::parse(MANIFEST_URL).expect("the manifest's address is a URL"),
        signature::built_in_key(),
    )
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
