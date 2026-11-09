//! What the host kept of its agent files before changing them, in floti.db's
//! `agent_versions`: one row per change a client made, which
//! `agents:restore` undoes newest first.
//!
//! The agent files are in the host's config folder, which is the user's; what
//! they were is kept in the host's data folder, never beside them. A row goes
//! in before its file is touched, and comes out again when the change to the
//! file fails, so every row is a change that happened. Each row also records
//! what the change left in its files, so that an undo puts a file back only
//! while it is as the change left it. The newest [`KEPT_PER_AGENT`] rows of
//! each agent are kept.

use rusqlite::{Connection, OptionalExtension, Row, params};

/// How many changes are kept for one agent.
pub const KEPT_PER_AGENT: u32 = 20;

/// What a change did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Wrote a file where there was none.
    Create,
    /// Wrote over a file.
    Replace,
    /// Wrote a file under a new name and removed the old one.
    Rename,
    /// Removed a file.
    Delete,
    /// Wrote a shipped agent where there was no file.
    AddShipped,
}

impl Reason {
    fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Replace => "replace",
            Self::Rename => "rename",
            Self::Delete => "delete",
            Self::AddShipped => "add-shipped",
        }
    }

    fn of(name: &str) -> Self {
        match name {
            "create" => Self::Create,
            "rename" => Self::Rename,
            "delete" => Self::Delete,
            "add-shipped" => Self::AddShipped,
            _ => Self::Replace,
        }
    }

    /// What the change did to the file, as a past participle: "deleted".
    pub fn done(self) -> &'static str {
        match self {
            Self::Create => "created",
            Self::Replace => "saved",
            Self::Rename => "renamed",
            Self::Delete => "deleted",
            Self::AddShipped => "added",
        }
    }
}

/// Where a rename wrote the file it renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renamed {
    /// The agent it became.
    pub name: String,
    /// Its file, such as `checker.md`.
    pub file: String,
    /// The hash of what was written there.
    pub hash: String,
}

/// One change, as it is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub id: i64,
    /// The agent changed; for a rename, the one renamed.
    pub name: String,
    /// Its file, such as `reviewer.md`.
    pub file: String,
    /// The file's text before the change; `None` when there was no file.
    pub text: Option<String>,
    /// The hash of what the change left in the file; `None` when it left
    /// none.
    pub after_hash: Option<String>,
    /// Where a rename wrote it.
    pub renamed: Option<Renamed>,
    pub reason: Reason,
}

/// Keeps `version` for the agents folder `folder`, before its change is
/// made, and answers its id. Its `id` is ignored.
///
/// # Errors
///
/// The sentence the change is refused with: the row could not go in.
pub fn keep(conn: &Connection, folder: &str, version: &Version) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO agent_versions
            (folder, name, file, text, after_hash, renamed_to, renamed_file, renamed_hash, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            folder,
            version.name,
            version.file,
            version.text,
            version.after_hash,
            version.renamed.as_ref().map(|renamed| &renamed.name),
            version.renamed.as_ref().map(|renamed| &renamed.file),
            version.renamed.as_ref().map(|renamed| &renamed.hash),
            version.reason.name(),
        ],
    )
    .map_err(|err| {
        format!(
            "Floti could not keep the text of {} before changing it, so it left the file as it was: {err}",
            version.file
        )
    })?;
    Ok(conn.last_insert_rowid())
}

/// Takes the row `id` out again: its change was not made, or was undone.
///
/// # Errors
///
/// A sentence when the row could not be taken out.
pub fn forget(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM agent_versions WHERE id = ?1", params![id])
        .map(drop)
        .map_err(|err| format!("Floti could not update what it keeps of the agent files: {err}"))
}

/// Takes out every row of `name` in `folder` but its newest
/// [`KEPT_PER_AGENT`].
///
/// # Errors
///
/// A sentence when the rows could not be taken out.
pub fn prune(conn: &Connection, folder: &str, name: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM agent_versions WHERE folder = ?1 AND name = ?2 AND id NOT IN (
            SELECT id FROM agent_versions WHERE folder = ?1 AND name = ?2
            ORDER BY id DESC LIMIT ?3)",
        params![folder, name, KEPT_PER_AGENT],
    )
    .map(drop)
    .map_err(|err| format!("Floti could not update what it keeps of the agent files: {err}"))
}

/// The newest change kept for `name` in `folder`: one made to its file, or a
/// rename that wrote it.
///
/// # Errors
///
/// A sentence when the rows could not be read.
pub fn newest(conn: &Connection, folder: &str, name: &str) -> Result<Option<Version>, String> {
    conn.query_row(
        "SELECT id, name, file, text, after_hash, renamed_to, renamed_file, renamed_hash, reason
         FROM agent_versions WHERE folder = ?1 AND (name = ?2 OR renamed_to = ?2)
         ORDER BY id DESC LIMIT 1",
        params![folder, name],
        version,
    )
    .optional()
    .map_err(|err| format!("Floti could not read what it keeps of the agent files: {err}"))
}

/// Every change kept for `name` in `folder`, newest first.
///
/// # Errors
///
/// A sentence when the rows could not be read.
pub fn all(conn: &Connection, folder: &str, name: &str) -> Result<Vec<Version>, String> {
    let read = || -> rusqlite::Result<Vec<Version>> {
        let mut statement = conn.prepare(
            "SELECT id, name, file, text, after_hash, renamed_to, renamed_file, renamed_hash, reason
             FROM agent_versions WHERE folder = ?1 AND name = ?2 ORDER BY id DESC",
        )?;
        statement
            .query_map(params![folder, name], version)?
            .collect()
    };
    read().map_err(|err| format!("Floti could not read what it keeps of the agent files: {err}"))
}

fn version(row: &Row<'_>) -> rusqlite::Result<Version> {
    let renamed_to: Option<String> = row.get("renamed_to")?;
    let renamed_file: Option<String> = row.get("renamed_file")?;
    let renamed_hash: Option<String> = row.get("renamed_hash")?;
    let reason: String = row.get("reason")?;
    Ok(Version {
        id: row.get("id")?,
        name: row.get("name")?,
        file: row.get("file")?,
        text: row.get("text")?,
        after_hash: row.get("after_hash")?,
        renamed: match (renamed_to, renamed_file, renamed_hash) {
            (Some(name), Some(file), Some(hash)) => Some(Renamed { name, file, hash }),
            _ => None,
        },
        reason: Reason::of(&reason),
    })
}
