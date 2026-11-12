//! The one `floti update` running on this computer, and how the last one
//! ended, kept in Floti's data folder so any process can tell.
//!
//! - **`update-run.json`** exists while an update runs. It is made with
//!   `create_new`, so it is also the lock: a second `floti update` (a
//!   terminal's, the host's background check, the one the desktop's Update
//!   starts) refuses while it names a process that is not known to have
//!   ended. One whose process is known to have ended, or that cannot be read
//!   as a record, is a leftover and is taken over.
//! - It holds the process's id and the last line the update said, marked
//!   while the update waits for the desktop app to quit. Each step rewrites
//!   it whole, renamed into place.
//! - **`update-ended.json`** is written when an update ends, with whether it
//!   ended well and its last line, and then the run file is removed. A host
//!   follows an update by reading the two (`server::features::update`),
//!   whether it started the update or found it running when it started.

use std::path::{Path, PathBuf};

use floti_protocol::Knowable;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::UpdateError;

/// The run file, in the data folder.
pub const RUN_FILE: &str = "update-run.json";

/// How the last update ended, in the data folder.
pub const ENDED_FILE: &str = "update-ended.json";

/// What the run file says: the update's process and its last line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub pid: u32,
    /// The last line it said, as a sentence.
    pub line: String,
    /// Everything is downloaded and checked, and it waits for the app to
    /// quit.
    #[serde(default)]
    pub waiting_for_app: bool,
}

/// What the ended file says: which process ended, when, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndedRecord {
    pub pid: u32,
    /// When it ended, as RFC 3339 text.
    pub at: String,
    /// Whether floti ended up to date or updated.
    pub ok: bool,
    /// Its last line: the failure line when it did not end well.
    pub line: String,
}

impl EndedRecord {
    /// When it ended; `None` when that cannot be read.
    pub fn ended_at(&self) -> Option<Timestamp> {
        self.at.parse().ok()
    }
}

/// What a look at the run file found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Look {
    /// An update says it runs.
    Running(RunRecord),
    /// No update runs.
    Absent,
    /// The file is there and could not be read now: mid-rename, or held
    /// by another process for an instant. Look again.
    Unreadable,
}

/// The run file in `data`, as it is now.
pub fn look(data: &Path) -> Look {
    match std::fs::read(data.join(RUN_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_or(Look::Unreadable, Look::Running),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Look::Absent,
        Err(_) => Look::Unreadable,
    }
}

/// How the last update in `data` ended, when that is kept.
pub fn ended(data: &Path) -> Option<EndedRecord> {
    let bytes = std::fs::read(data.join(ENDED_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The run file this process holds while its update runs. Dropped without
/// [`RunLock::end`], it removes the run file.
#[derive(Debug)]
pub struct RunLock {
    data: PathBuf,
    record: RunRecord,
    ended: bool,
}

impl RunLock {
    /// Makes the run file in `data` for process `pid`, saying `line`;
    /// refused while another update runs. A run file whose process
    /// `is_alive` says has ended, or that is no record, is taken over.
    pub fn take(
        data: &Path,
        pid: u32,
        line: &str,
        is_alive: &dyn Fn(u32) -> Knowable<bool>,
    ) -> Result<Self, UpdateError> {
        let disk = |err: std::io::Error| UpdateError::Disk {
            reason: format!("could not make {}: {err}", data.join(RUN_FILE).display()),
        };
        std::fs::create_dir_all(data).map_err(disk)?;
        let record = RunRecord {
            pid,
            line: line.to_owned(),
            waiting_for_app: false,
        };
        let file = data.join(RUN_FILE);
        // Twice at most: a leftover is removed once, and a second update
        // that made the file meanwhile wins.
        for _ in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)
            {
                Ok(mut made) => {
                    use std::io::Write;
                    let text = serde_json::to_vec(&record).unwrap_or_default();
                    made.write_all(&text).map_err(disk)?;
                    return Ok(Self {
                        data: data.to_path_buf(),
                        record,
                        ended: false,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    match look(data) {
                        Look::Running(other)
                            if !matches!(is_alive(other.pid), Knowable::Known(false)) =>
                        {
                            return Err(UpdateError::AlreadyRunning { pid: other.pid });
                        }
                        // A leftover: its process has ended, or it is no record.
                        Look::Running(_) | Look::Unreadable => {
                            let _ = std::fs::remove_file(&file);
                        }
                        Look::Absent => {}
                    }
                }
                Err(err) => return Err(disk(err)),
            }
        }
        match look(data) {
            Look::Running(other) => Err(UpdateError::AlreadyRunning { pid: other.pid }),
            _ => Err(UpdateError::Disk {
                reason: format!("could not make {}", file.display()),
            }),
        }
    }

    /// Says `line`, marked as waiting for the app when `waiting_for_app`.
    /// A step that cannot be written changes nothing in the update.
    pub fn step(&mut self, line: &str, waiting_for_app: bool) {
        line.clone_into(&mut self.record.line);
        self.record.waiting_for_app = waiting_for_app;
        let _ = write_whole(&self.data, RUN_FILE, &self.record);
    }

    /// Ends the update at `at`: well with `Ok(line)`, or with the failure
    /// line. Keeps how it ended, then removes the run file.
    pub fn end(mut self, at: Timestamp, how: Result<&str, &str>) {
        let (ok, line) = match how {
            Ok(line) => (true, line),
            Err(line) => (false, line),
        };
        let _ = write_whole(
            &self.data,
            ENDED_FILE,
            &EndedRecord {
                pid: self.record.pid,
                at: at.to_string(),
                ok,
                line: line.to_owned(),
            },
        );
        self.ended = true;
        let _ = std::fs::remove_file(self.data.join(RUN_FILE));
    }
}

impl Drop for RunLock {
    fn drop(&mut self) {
        if !self.ended {
            let _ = std::fs::remove_file(self.data.join(RUN_FILE));
        }
    }
}

/// Writes `value` as `name` in `data`, whole, renamed into place.
fn write_whole(data: &Path, name: &str, value: &impl Serialize) -> std::io::Result<()> {
    let partial = data.join(format!("{name}.{}.partial", std::process::id()));
    std::fs::write(&partial, serde_json::to_vec(value).unwrap_or_default())?;
    std::fs::rename(&partial, data.join(name)).inspect_err(|_| {
        let _ = std::fs::remove_file(&partial);
    })
}

#[cfg(test)]
mod tests;
