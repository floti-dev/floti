//! The host keeping floti up to date by itself: `floti update`'s checks for
//! floti alone, at most once a day, while the host's `updateCheck` setting
//! is on (on by default; Settings → Updates turns it off).
//!
//! - Only a host running floti installed on its own ([`eligible`]) does it:
//!   one run from a development build, from inside the desktop app, or with
//!   a config folder that is another host's (`FLOTI_CONFIG_READ_ONLY`, every
//!   development run and check) never looks.
//! - A check reads the signed manifest and, when the channel has a newer
//!   floti, downloads and checks it exactly as `floti update` does
//!   ([`Updater`] with no desktop step), then puts it in place
//!   ([`Standalone::replace`]). The desktop app is not touched.
//! - Once a newer floti is in place the host relaunches on it, without
//!   waiting for any session to end: it lets go of its session keeper
//!   ([`HostStopReason::Relaunch`]), so every session and plain terminal runs
//!   on, starts the new floti in its place and exits, and the new host takes
//!   the keeper back. A new host that does not speak the keeper's link
//!   version ends it with what it holds, and those sessions come back
//!   through the restore offer in their tabs.
//! - A client may ask for a check now (`update:check`): it runs at once,
//!   whatever the setting says, and the client is answered once it has
//!   ended. Every check is told as it starts and ends ([`Told`]), for the
//!   host to tell its clients (`update:state`).
//! - When a check reached the manifest, the time and the channel's newest
//!   version are kept in `update-checked.json` in the data folder, so a
//!   restarted or relaunched host does not look again within the day and
//!   knows what that check found. A check that could not reach
//!   the release (no network, a download cut off, a disk refusing the file)
//!   is tried again after [`RETRY_AFTER`]; one the release itself failed (a
//!   signature, a checksum, a manifest without this computer's build) waits
//!   the day.
//!
//! [`Standalone::replace`]: crate::platform::standalone::Standalone::replace
//! [`HostStopReason::Relaunch`]: floti_protocol::channels::host::HostStopReason::Relaunch

use std::path::{Path, PathBuf};
use std::time::Duration;

use floti_protocol::channels::update::UpdateUnavailable;
use jiff::{SignedDuration, Timestamp};
use tokio::sync::{mpsc, oneshot, watch};

use super::{UpdateError, Updater, network, signature};
use crate::platform::folders::UserEnv;
use crate::platform::installs;
use crate::platform::process_table;
use crate::platform::process_tree::{self, ProcessTree};
use crate::platform::standalone::{self, Standalone};
use crate::version::{FLOTI_VERSION, cpu_name, os_name};

/// How long after a check that reached the manifest the next one is due.
pub const CHECK_EVERY: SignedDuration = SignedDuration::from_hours(24);

/// How long after a check that could not reach the release it is tried
/// again.
pub const RETRY_AFTER: SignedDuration = SignedDuration::from_hours(1);

/// The file in the data folder that keeps when a check last reached the
/// manifest, and the channel's newest version it found:
/// `{"at": "<RFC 3339>", "newest": "<version>"}`.
pub const CHECKED_FILE: &str = "update-checked.json";

/// When the next check is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// The setting is off: none until it is turned on.
    Off,
    /// Now.
    Now,
    /// At this time, unless the setting changes first.
    At(Timestamp),
}

/// When the next check is, with the setting `on`, the last check that
/// reached the manifest at `checked`, the last attempt at `tried` and the
/// time `now`. A time kept in the future, from a clock set back, counts as
/// now, so it never holds updates off for longer than a day.
pub fn next(
    on: bool,
    checked: Option<Timestamp>,
    tried: Option<Timestamp>,
    now: Timestamp,
) -> Next {
    if !on {
        return Next::Off;
    }
    let after = |at: Option<Timestamp>, wait: SignedDuration| {
        at.map(|at| at.min(now))
            .and_then(|at| at.checked_add(wait).ok())
    };
    match after(checked, CHECK_EVERY).max(after(tried, RETRY_AFTER)) {
        Some(due) if due > now => Next::At(due),
        _ => Next::Now,
    }
}

/// How one check ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// The channel has nothing newer than this floti: its newest is
    /// `newest`.
    UpToDate { newest: String },
    /// A newer floti is in place: the host relaunches on it.
    Replaced { version: String },
    /// It failed, with the line saying why. `soon` when it could not reach
    /// the release, so it is tried again after [`RETRY_AFTER`]; otherwise the
    /// release itself failed and the next look waits the day.
    Failed { why: String, soon: bool },
}

/// Why the background update ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// A newer floti is in place, so the host relaunches on it.
    Relaunch,
    /// The settings are gone: the host is stopping.
    Stopping,
}

impl Checked {
    /// The channel's newest version, when the check found it.
    pub fn newest(&self) -> Option<&str> {
        match self {
            Self::UpToDate { newest } => Some(newest),
            Self::Replaced { version } => Some(version),
            Self::Failed { .. } => None,
        }
    }
}

/// A check starting or ending, as the loop tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Told {
    /// A check started.
    Checking,
    /// A check that started at `at` ended.
    Checked { at: Timestamp, checked: Checked },
}

/// A client asking for a check now, answered once the check has ended.
pub type Ask = oneshot::Sender<()>;

/// Why a host whose config folder is read-only or that runs any floti but
/// the one installed on its own does not update: what its clients are told.
pub fn unavailable(config_read_only: bool) -> UpdateUnavailable {
    if config_read_only {
        UpdateUnavailable::ReadOnly
    } else {
        UpdateUnavailable::NotStandalone
    }
}

/// The floti this host keeps up to date: `exe`, the one it runs, when that
/// is floti installed on its own for the user `env` describes and the
/// host's config folder is its own; `None` otherwise.
pub fn eligible(env: &UserEnv, exe: Option<&Path>, config_read_only: bool) -> Option<PathBuf> {
    if config_read_only {
        return None;
    }
    let exe = exe?;
    let standalone = standalone::Native.binary(env)?;
    let same = match (
        std::fs::canonicalize(exe),
        std::fs::canonicalize(&standalone),
    ) {
        (Ok(exe), Ok(standalone)) => exe == standalone,
        _ => false,
    };
    same.then(|| exe.to_path_buf())
}

/// What the last check that reached the manifest kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    /// When it reached the manifest.
    pub at: Timestamp,
    /// The channel's newest version it found, when it kept one.
    pub newest: Option<String>,
}

/// What the last check that reached the manifest kept in `data`; `None`
/// when none has or the file cannot be read.
pub fn kept(data: &Path) -> Option<Kept> {
    let text = std::fs::read_to_string(data.join(CHECKED_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Kept {
        at: value["at"].as_str()?.parse().ok()?,
        newest: value["newest"].as_str().map(str::to_owned),
    })
}

/// When a check last reached the manifest, as kept in `data`; `None` when
/// none has or the file cannot be read.
pub fn last_checked(data: &Path) -> Option<Timestamp> {
    kept(data).map(|kept| kept.at)
}

/// Keeps `at` as when a check last reached the manifest, and `newest` as
/// what it found when it found it, written whole and renamed into place.
pub fn remember_checked(data: &Path, at: Timestamp, newest: Option<&str>) -> std::io::Result<()> {
    std::fs::create_dir_all(data)?;
    let file = data.join(CHECKED_FILE);
    let partial = data.join(format!("{CHECKED_FILE}.{}.partial", std::process::id()));
    let mut kept = serde_json::json!({ "at": at.to_string() });
    if let Some(newest) = newest {
        kept["newest"] = newest.into();
    }
    std::fs::write(&partial, kept.to_string())?;
    std::fs::rename(&partial, &file).inspect_err(|_| {
        let _ = std::fs::remove_file(&partial);
    })
}

/// Checks whenever [`next`] says, following the setting on `on`, and at
/// once for each ask on `asks`, until a newer floti is in place or the
/// setting's sender is gone. Each check is told to `told` as it starts and
/// ends, and every ask that came before it ended is answered then. `clock`
/// says the time, and `check` runs one check, on a blocking thread; tests
/// hand it fakes.
pub async fn keep_up_to_date<C>(
    data: PathBuf,
    mut on: watch::Receiver<bool>,
    mut asks: mpsc::UnboundedReceiver<Ask>,
    clock: impl Fn() -> Timestamp,
    check: C,
    told: impl Fn(Told),
) -> Ended
where
    C: Fn() -> Checked + Clone + Send + 'static,
{
    let mut tried = None;
    let mut waiting: Vec<Ask> = Vec::new();
    // Once every sender is gone no ask can come, and `recv` would answer
    // `None` at once on every turn.
    let mut asks_open = true;
    loop {
        let enabled = *on.borrow_and_update();
        let now = clock();
        let due = if waiting.is_empty() {
            next(enabled, last_checked(&data), tried, now)
        } else {
            Next::Now
        };
        let wait = match due {
            Next::Off => None,
            Next::At(due) => Some(Duration::try_from(due.duration_since(now)).unwrap_or_default()),
            Next::Now => {
                tried = Some(now);
                told(Told::Checking);
                let check = check.clone();
                let checked = tokio::task::spawn_blocking(check)
                    .await
                    .unwrap_or_else(|err| Checked::Failed {
                        why: format!("the check stopped: {err}"),
                        soon: true,
                    });
                let reached = !matches!(checked, Checked::Failed { soon: true, .. });
                if reached && let Err(err) = remember_checked(&data, now, checked.newest()) {
                    log_line!("floti host: the update check's time was not kept: {err}");
                }
                told(Told::Checked {
                    at: now,
                    checked: checked.clone(),
                });
                // An ask that came while this check ran is answered by it.
                while let Ok(ask) = asks.try_recv() {
                    waiting.push(ask);
                }
                for ask in waiting.drain(..) {
                    let _ = ask.send(());
                }
                match checked {
                    Checked::UpToDate { .. } => {}
                    Checked::Replaced { version } => {
                        log_line!(
                            "floti host: floti {version} is in place; the host starts it in its place, and its sessions run on."
                        );
                        return Ended::Relaunch;
                    }
                    Checked::Failed { why, .. } => {
                        log_line!("floti host: the update check failed: {why}");
                    }
                }
                continue;
            }
        };
        tokio::select! {
            () = sleep_or_forever(wait) => {}
            changed = on.changed() => {
                if changed.is_err() {
                    return Ended::Stopping;
                }
            }
            ask = asks.recv(), if asks_open => match ask {
                Some(ask) => waiting.push(ask),
                None => asks_open = false,
            },
        }
    }
}

/// Sleeps for `wait`, or never with none.
async fn sleep_or_forever(wait: Option<Duration>) {
    match wait {
        Some(wait) => tokio::time::sleep(wait).await,
        None => std::future::pending().await,
    }
}

/// Whether `err` is the network, the disk or another update running, which
/// a later look may get past, rather than the release itself.
pub fn retries_soon(err: &UpdateError) -> bool {
    matches!(
        err,
        UpdateError::Network { .. }
            | UpdateError::CutOff { .. }
            | UpdateError::Disk { .. }
            | UpdateError::Replace { .. }
            | UpdateError::AlreadyRunning { .. }
    )
}

/// One real check, by the host on `data`, of floti at `binary`: what
/// `floti update` does, for floti alone. Each line it says goes to the log.
pub fn check_now(data: &Path, binary: &Path) -> Checked {
    let network = match network::Https::new() {
        Ok(network) => network,
        Err(why) => return Checked::Failed { why, soon: true },
    };
    let key = signature::trusted_key(data);
    let is_alive = |pid: u32| process_tree::Native.is_alive(pid);
    let install_self =
        |_: &Path| -> Result<String, String> { Err("the host updates floti alone".to_owned()) };
    let pause = |_: Duration| {};
    let mut said = Vec::new();
    let outcome = Updater {
        running_version: FLOTI_VERSION,
        target: format!("{}-{}", os_name(), cpu_name()),
        manifest_url: super::manifest_url(data),
        key: &key,
        network: &network,
        installs: &installs::Native,
        standalone: &standalone::Native,
        install_self: &install_self,
        processes: &process_table::Native,
        is_alive: &is_alive,
        relaunch_host: None,
        with_desktop: false,
        pause: &pause,
        env: UserEnv::from_process(),
        running_binary: binary.to_path_buf(),
        data: data.to_path_buf(),
        chosen_channel: None,
        from_desktop: None,
        out: &mut said,
        held: None,
    }
    .run();
    for line in String::from_utf8_lossy(&said).lines() {
        log_line!("floti host: {line}");
    }
    match outcome {
        Ok(super::Outcome::Updated {
            version,
            floti: true,
            ..
        }) => Checked::Replaced { version },
        Ok(super::Outcome::Updated { version, .. }) => Checked::UpToDate { newest: version },
        Ok(super::Outcome::UpToDate { newest, .. }) => Checked::UpToDate { newest },
        Err(err) => Checked::Failed {
            soon: retries_soon(&err),
            why: super::failure_line(&err),
        },
    }
}

/// Removes the copies of `binary` an earlier update put aside, once nothing
/// runs them: what the host this one replaced ran from, now that it has
/// ended.
pub fn sweep_asides(binary: &Path) {
    let removed = standalone::Native.remove_asides(binary);
    if !removed.is_empty() {
        log_line!(
            "floti host: removed {} copies of floti an earlier update put aside.",
            removed.len()
        );
    }
}

#[cfg(test)]
mod tests;
