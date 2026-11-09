//! The agents folder as a client reads and edits it: `agents:read`,
//! `agents:write`, `agents:delete`, `agents:restore` and `agents:addShipped`.
//!
//! Four rules, in this order:
//!
//! 1. A host whose config folder is another host's to write changes nothing
//!    there ([`READ_ONLY`]).
//! 2. A text is parsed by the parser a start uses before anything is
//!    written, and one it refuses is refused with its problems.
//! 3. Nothing is written when the file is not what the client read: the
//!    hash it names is the file's now, or it named none and there is no
//!    file. That is an outside edit, answered as a conflict rather than
//!    written over, as `content:write` answers one.
//! 4. Nothing is replaced, renamed or removed until what it was is kept in
//!    floti.db ([`versions`]), and a change whose row cannot go in does not
//!    happen. A restore undoes the newest change kept, only while its files
//!    are as that change left them, so it never discards an edit made since.
//!
//! A file is replaced through a file beside it renamed over it, so a start
//! reading the folder meanwhile reads the old text or the new, never half of
//! one. A new file is written only where there is none, so nothing a client
//! did not read is ever written over.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use floti_protocol::channels::agents::{
    AgentChange, AgentConflict, AgentFields, AgentFile, AgentProblem, AgentUnusedLine,
    AgentWrittenValue, DeleteAgentRequest, WriteAgentRequest,
};
use rusqlite::Connection;

use super::folder::NOT_TEXT;
use super::parse::{AgentFileReading, DefinitionProblem, read_agent_file};
use super::seed::SHIPPED_AGENTS;
use super::versions::{self, Reason, Renamed, Version};
use super::{AgentDefinition, DEFAULT_AGENT_NAME, FolderDefinitions, is_default_name};
use crate::config_console::sha256_hex;

/// Why a host whose config folder is another host's to write refuses every
/// change to its agents.
pub const READ_ONLY: &str = "This host does not change its config folder, so it changes no agent: it is a development or check run, whose config folder can be another host's. Its agents are listed, read and watched here.";

/// A file in the agents folder as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    Absent,
    Present {
        /// Its text; `None` when it is not UTF-8.
        text: Option<String>,
        /// The SHA-256 of its bytes.
        hash: String,
    },
}

impl FileState {
    /// The file at `path` now.
    ///
    /// # Errors
    ///
    /// The file is there and could not be read.
    pub fn of(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Self::Present {
                hash: sha256_hex(&bytes),
                text: String::from_utf8(bytes).ok(),
            }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self::Absent),
            Err(err) => Err(err),
        }
    }

    pub fn hash(&self) -> Option<&str> {
        match self {
            Self::Absent => None,
            Self::Present { hash, .. } => Some(hash),
        }
    }

    fn conflict(&self) -> AgentConflict {
        match self {
            Self::Absent => AgentConflict {
                on_disk_hash: None,
                on_disk_text: None,
            },
            Self::Present { text, hash } => AgentConflict {
                on_disk_hash: Some(hash.clone()),
                on_disk_text: text.clone(),
            },
        }
    }
}

/// The name an agent is kept and answered under: the default's for either of
/// its names.
pub fn canonical(name: &str) -> &str {
    if is_default_name(name) {
        DEFAULT_AGENT_NAME
    } else {
        name
    }
}

/// One agent file, as `agents:read` answers it.
pub fn agent_file(name: &str, file: &str, state: &FileState, in_folder: bool) -> Option<AgentFile> {
    let FileState::Present { text, hash } = state else {
        return None;
    };
    let name = canonical(name);
    let Some(text) = text else {
        return Some(AgentFile {
            name: name.to_owned(),
            file: file.to_owned(),
            in_folder,
            text: String::new(),
            hash: hash.clone(),
            values: Vec::new(),
            fields: None,
            body: String::new(),
            problems: vec![problem(&DefinitionProblem::whole(NOT_TEXT))],
            unused_lines: Vec::new(),
        });
    };
    let reading = read_agent_file(name, text);
    let (fields, problems, unused_lines) = match &reading.agent {
        Ok(agent) => (
            Some(fields(agent)),
            Vec::new(),
            unused_lines(agent, &reading),
        ),
        Err(problems) => (None, problems.iter().map(problem).collect(), Vec::new()),
    };
    Some(AgentFile {
        name: name.to_owned(),
        file: file.to_owned(),
        in_folder,
        text: text.clone(),
        hash: hash.clone(),
        values: reading
            .values
            .iter()
            .map(|value| AgentWrittenValue {
                field: value.field.clone(),
                line: value.line,
                value: value.value.clone(),
            })
            .collect(),
        fields,
        body: reading.body.clone(),
        problems,
        unused_lines,
    })
}

/// A problem, as a client is told it.
pub fn problem(problem: &DefinitionProblem) -> AgentProblem {
    AgentProblem {
        field: problem.field.clone(),
        line: problem.line,
        message: problem.message.clone(),
    }
}

/// The retired lines `agent`'s file has, each with the line it is on.
pub fn unused_lines(agent: &AgentDefinition, reading: &AgentFileReading) -> Vec<AgentUnusedLine> {
    agent
        .unused_lines
        .iter()
        .filter_map(|field| {
            Some(AgentUnusedLine {
                field: field.clone(),
                line: reading.line_of(field)?,
            })
        })
        .collect()
}

/// An agent's fields, as a client is told them.
pub fn fields(agent: &AgentDefinition) -> AgentFields {
    AgentFields {
        description: agent.description.clone(),
        profile: agent.profile.clone(),
        backends: agent
            .backends
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect(),
        models: agent.models.clone(),
        efforts: agent.efforts.clone(),
        permission: agent.permission,
        worktree: agent.worktree,
        can_start: agent.can_start,
        allowed_tools: agent.allowed_tools.clone(),
        disallowed_tools: agent.disallowed_tools.clone(),
        command: agent.command.clone(),
        submit: agent.submit.clone(),
        stop: agent.stop.clone(),
    }
}

/// The shipped agent `name`, as `(file, text)`.
pub fn shipped(name: &str) -> Option<(&'static str, &'static str)> {
    SHIPPED_AGENTS
        .iter()
        .find(|(file, _)| file.strip_suffix(".md") == Some(name))
        .copied()
}

/// The names of the shipped agents, as a sentence's list.
fn shipped_names() -> String {
    let names: Vec<&str> = SHIPPED_AGENTS
        .iter()
        .filter_map(|(file, _)| file.strip_suffix(".md"))
        .collect();
    match names.split_last() {
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// A change that was not made, and why.
fn refused(refusal: String, file: Option<AgentFile>) -> AgentChange {
    AgentChange {
        changed: false,
        file,
        refusal: Some(refusal),
        problems: Vec::new(),
        conflict: None,
    }
}

/// Reads and changes the agents in one folder, keeping what it replaces in
/// one database.
pub struct AgentEditor<'a> {
    folder: &'a FolderDefinitions,
    conn: &'a Connection,
    read_only: bool,
}

impl<'a> AgentEditor<'a> {
    /// The agents in `folder`, with what they were kept in `conn`. A
    /// `read_only` editor changes nothing.
    pub fn new(folder: &'a FolderDefinitions, conn: &'a Connection, read_only: bool) -> Self {
        Self {
            folder,
            conn,
            read_only,
        }
    }

    fn dir(&self) -> &Path {
        self.folder.dir()
    }

    /// The folder as the kept rows name it.
    fn key(&self) -> String {
        self.dir().display().to_string()
    }

    /// The file of the agent `name`, or the sentence for a name that is no
    /// agent's.
    fn file_of(&self, name: &str) -> Result<String, String> {
        self.folder.file_of(name).ok_or_else(|| {
            format!(
                "{name} is not an agent name: an agent's name is lowercase letters, digits and dashes."
            )
        })
    }

    fn path(&self, file: &str) -> PathBuf {
        self.dir().join(file)
    }

    fn state(&self, file: &str) -> Result<FileState, String> {
        let path = self.path(file);
        FileState::of(&path)
            .map_err(|err| format!("Floti could not read {}: {err}", path.display()))
    }

    /// The agent `name`'s file as it is now, or `None` when there is none.
    fn current(&self, name: &str, file: &str) -> Result<Option<AgentFile>, String> {
        Ok(agent_file(name, file, &self.state(file)?, true))
    }

    /// One agent's file. A host serving the shipped agents while it has no
    /// folder answers a shipped one's text.
    ///
    /// # Errors
    ///
    /// A sentence for a name that is no agent's, an agent with no file, or a
    /// file that could not be read.
    pub fn read(&self, name: &str) -> Result<AgentFile, String> {
        let file = self.file_of(name)?;
        if self.folder.serves_shipped()
            && let Some((shipped_file, text)) = shipped(canonical(name))
        {
            let state = FileState::Present {
                text: Some(text.to_owned()),
                hash: sha256_hex(text.as_bytes()),
            };
            return agent_file(name, shipped_file, &state, false)
                .ok_or_else(|| format!("The shipped agent {shipped_file} could not be read."));
        }
        self.current(name, &file)?.ok_or_else(|| {
            if is_default_name(name) {
                format!(
                    "The default agent has no file in {}: it is made from the installed backends. Writing {DEFAULT_AGENT_NAME}.md gives it one.",
                    self.dir().display()
                )
            } else {
                format!(
                    "There is no agent file {file} in {}.",
                    self.dir().display()
                )
            }
        })
    }

    /// Writes `request`'s text as its agent's whole file, creating, replacing
    /// or renaming it.
    ///
    /// # Errors
    ///
    /// A sentence when the folder or the database could not be read or
    /// written; every refusal is an answer.
    pub fn write(&self, request: &WriteAgentRequest) -> Result<AgentChange, String> {
        let name = canonical(&request.name).to_owned();
        let file = match self.file_of(&request.name) {
            Ok(file) => file,
            Err(refusal) => return Ok(refused(refusal, None)),
        };
        if self.read_only {
            return Ok(refused(READ_ONLY.to_owned(), self.current(&name, &file)?));
        }
        let from = match request.rename_from.as_deref() {
            Some(from) if canonical(from) != name => Some(match self.file_of(from) {
                Ok(from_file) => (canonical(from).to_owned(), from_file),
                Err(refusal) => return Ok(refused(refusal, None)),
            }),
            _ => None,
        };
        let target = self.state(&file)?;
        if let Err(problems) = read_agent_file(&name, &request.text).agent {
            return Ok(AgentChange {
                changed: false,
                file: agent_file(&name, &file, &target, true),
                refusal: Some(format!(
                    "{file} was not saved, because the parser a start uses refuses it: {}.",
                    super::parse::joined(&problems)
                )),
                problems: problems.iter().map(problem).collect(),
                conflict: None,
            });
        }
        match from {
            Some((from_name, from_file)) => {
                self.rename(request, (&from_name, &from_file), (&name, &file), &target)
            }
            None => self.replace(request, &name, &file, &target),
        }
    }

    fn replace(
        &self,
        request: &WriteAgentRequest,
        name: &str,
        file: &str,
        target: &FileState,
    ) -> Result<AgentChange, String> {
        if target.hash() != request.expected_hash.as_deref() {
            return Ok(conflicted(name, file, target));
        }
        let before = match target {
            FileState::Absent => None,
            FileState::Present {
                text: Some(text), ..
            } => {
                if *text == request.text {
                    return Ok(AgentChange {
                        changed: false,
                        file: agent_file(name, file, target, true),
                        refusal: None,
                        problems: Vec::new(),
                        conflict: None,
                    });
                }
                Some(text.clone())
            }
            FileState::Present { text: None, .. } => {
                return Ok(refused(
                    not_text(file),
                    agent_file(name, file, target, true),
                ));
            }
        };
        let reason = if before.is_some() {
            Reason::Replace
        } else {
            Reason::Create
        };
        let id = versions::keep(
            self.conn,
            &self.key(),
            &Version {
                id: 0,
                name: name.to_owned(),
                file: file.to_owned(),
                text: before.clone(),
                after_hash: Some(sha256_hex(request.text.as_bytes())),
                renamed: None,
                reason,
            },
        )?;
        let path = self.path(file);
        let written = if before.is_some() {
            replace_file(&path, &request.text)
        } else {
            create_file(&path, &request.text)
        };
        if let Err(err) = written {
            versions::forget(self.conn, id)?;
            // A file that appeared since it was read is an outside edit.
            if err.kind() == io::ErrorKind::AlreadyExists {
                return Ok(conflicted(name, file, &self.state(file)?));
            }
            return Err(format!("Floti could not write {}: {err}", path.display()));
        }
        self.changed(name, file)
    }

    fn rename(
        &self,
        request: &WriteAgentRequest,
        (from_name, from_file): (&str, &str),
        (name, file): (&str, &str),
        target: &FileState,
    ) -> Result<AgentChange, String> {
        let source = self.state(from_file)?;
        if source.hash() != request.expected_hash.as_deref() || source == FileState::Absent {
            return Ok(conflicted(from_name, from_file, &source));
        }
        if *target != FileState::Absent {
            return Ok(refused(
                format!(
                    "{from_file} was not renamed: there is already an agent file {file}, and a rename never writes over one."
                ),
                agent_file(from_name, from_file, &source, true),
            ));
        }
        let FileState::Present {
            text: Some(before), ..
        } = &source
        else {
            return Ok(refused(
                not_text(from_file),
                agent_file(from_name, from_file, &source, true),
            ));
        };
        let id = versions::keep(
            self.conn,
            &self.key(),
            &Version {
                id: 0,
                name: from_name.to_owned(),
                file: from_file.to_owned(),
                text: Some(before.clone()),
                after_hash: None,
                renamed: Some(Renamed {
                    name: name.to_owned(),
                    file: file.to_owned(),
                    hash: sha256_hex(request.text.as_bytes()),
                }),
                reason: Reason::Rename,
            },
        )?;
        let path = self.path(file);
        if let Err(err) = create_file(&path, &request.text) {
            versions::forget(self.conn, id)?;
            return match err.kind() {
                io::ErrorKind::AlreadyExists => Ok(refused(
                    format!(
                        "{from_file} was not renamed: there is already an agent file {file}, and a rename never writes over one."
                    ),
                    agent_file(from_name, from_file, &source, true),
                )),
                _ => Err(format!("Floti could not write {}: {err}", path.display())),
            };
        }
        let old = self.path(from_file);
        if let Err(err) = std::fs::remove_file(&old) {
            // Back to how it was: the new file goes again, and nothing is
            // kept of a change that did not happen.
            if let Err(undo) = std::fs::remove_file(&path) {
                log_line!(
                    "floti host: {} was left beside {} after a rename failed: {undo}",
                    path.display(),
                    old.display()
                );
            }
            versions::forget(self.conn, id)?;
            return Err(format!(
                "Floti could not rename {}, so it left it as it was: {err}",
                old.display()
            ));
        }
        versions::prune(self.conn, &self.key(), from_name)?;
        self.changed(name, file)
    }

    /// Removes an agent's file, keeping its text first.
    ///
    /// # Errors
    ///
    /// A sentence when the folder or the database could not be read or
    /// written; every refusal is an answer.
    pub fn delete(&self, request: &DeleteAgentRequest) -> Result<AgentChange, String> {
        let name = canonical(&request.name).to_owned();
        let file = match self.file_of(&request.name) {
            Ok(file) => file,
            Err(refusal) => return Ok(refused(refusal, None)),
        };
        let current = self.state(&file)?;
        if self.read_only {
            return Ok(refused(
                READ_ONLY.to_owned(),
                agent_file(&name, &file, &current, true),
            ));
        }
        let text = match &current {
            FileState::Absent => {
                return Ok(match request.expected_hash {
                    Some(_) => conflicted(&name, &file, &current),
                    None => refused(
                        format!("There is no agent file {file} in {}.", self.dir().display()),
                        None,
                    ),
                });
            }
            FileState::Present { hash, .. }
                if request
                    .expected_hash
                    .as_ref()
                    .is_some_and(|expected| expected != hash) =>
            {
                return Ok(conflicted(&name, &file, &current));
            }
            FileState::Present { text: None, .. } => {
                return Ok(refused(
                    not_text(&file),
                    agent_file(&name, &file, &current, true),
                ));
            }
            FileState::Present {
                text: Some(text), ..
            } => text.clone(),
        };
        let id = versions::keep(
            self.conn,
            &self.key(),
            &Version {
                id: 0,
                name: name.clone(),
                file: file.clone(),
                text: Some(text),
                after_hash: None,
                renamed: None,
                reason: Reason::Delete,
            },
        )?;
        let path = self.path(&file);
        if let Err(err) = std::fs::remove_file(&path) {
            versions::forget(self.conn, id)?;
            return Err(format!(
                "Floti could not remove {}, so it left it as it was: {err}",
                path.display()
            ));
        }
        versions::prune(self.conn, &self.key(), &name)?;
        Ok(AgentChange {
            changed: true,
            file: None,
            refusal: None,
            problems: Vec::new(),
            conflict: None,
        })
    }

    /// Undoes the newest change kept for `name`.
    ///
    /// # Errors
    ///
    /// A sentence when the folder or the database could not be read or
    /// written; every refusal is an answer.
    pub fn restore(&self, name: &str) -> Result<AgentChange, String> {
        let asked = canonical(name).to_owned();
        let file = match self.file_of(name) {
            Ok(file) => file,
            Err(refusal) => return Ok(refused(refusal, None)),
        };
        if self.read_only {
            return Ok(refused(READ_ONLY.to_owned(), self.current(&asked, &file)?));
        }
        let Some(version) = versions::newest(self.conn, &self.key(), &asked)? else {
            return Ok(refused(
                format!("Floti has kept nothing of {asked} to put back."),
                self.current(&asked, &file)?,
            ));
        };
        let main = self.state(&version.file)?;
        if main.hash() != version.after_hash.as_deref() {
            return Ok(changed_since(&version, &version.name, &version.file, &main));
        }
        let renamed = match &version.renamed {
            Some(renamed) => {
                let state = self.state(&renamed.file)?;
                if state.hash() != Some(renamed.hash.as_str()) {
                    return Ok(changed_since(
                        &version,
                        &renamed.name,
                        &renamed.file,
                        &state,
                    ));
                }
                Some(renamed)
            }
            None => None,
        };
        let path = self.path(&version.file);
        let put_back = match &version.text {
            Some(text) if main == FileState::Absent => create_file(&path, text),
            Some(text) => replace_file(&path, text),
            None => std::fs::remove_file(&path),
        };
        put_back.map_err(|err| {
            format!(
                "Floti could not put {} back, so it left it as it was: {err}",
                path.display()
            )
        })?;
        if let Some(renamed) = renamed {
            let gone = self.path(&renamed.file);
            if let Err(err) = std::fs::remove_file(&gone) {
                // Back to how it was before the restore: the rename stands.
                if let Err(undo) = std::fs::remove_file(&path) {
                    log_line!(
                        "floti host: {} was left beside {} after a restore failed: {undo}",
                        path.display(),
                        gone.display()
                    );
                }
                return Err(format!(
                    "Floti could not remove {} to undo the rename, so it left both as they were: {err}",
                    gone.display()
                ));
            }
        }
        versions::forget(self.conn, version.id)?;
        Ok(AgentChange {
            changed: true,
            file: self.current(&version.name, &version.file)?,
            refusal: None,
            problems: Vec::new(),
            conflict: None,
        })
    }

    /// Writes the shipped agent `name` where the folder has no file of that
    /// name.
    ///
    /// # Errors
    ///
    /// A sentence when the folder or the database could not be read or
    /// written; every refusal is an answer.
    pub fn add_shipped(&self, name: &str) -> Result<AgentChange, String> {
        let Some((file, text)) = shipped(name) else {
            return Ok(refused(
                format!(
                    "There is no shipped agent called {name}: the shipped agents are {}.",
                    shipped_names()
                ),
                None,
            ));
        };
        let current = self.state(file)?;
        if self.read_only {
            return Ok(refused(
                READ_ONLY.to_owned(),
                agent_file(name, file, &current, true),
            ));
        }
        let already = || {
            format!(
                "{file} is already in the agents folder, and adding the shipped agent never writes over it."
            )
        };
        if current != FileState::Absent {
            return Ok(refused(already(), agent_file(name, file, &current, true)));
        }
        let id = versions::keep(
            self.conn,
            &self.key(),
            &Version {
                id: 0,
                name: name.to_owned(),
                file: file.to_owned(),
                text: None,
                after_hash: Some(sha256_hex(text.as_bytes())),
                renamed: None,
                reason: Reason::AddShipped,
            },
        )?;
        let path = self.path(file);
        if let Err(err) = create_file(&path, text) {
            versions::forget(self.conn, id)?;
            return match err.kind() {
                io::ErrorKind::AlreadyExists => Ok(refused(already(), self.current(name, file)?)),
                _ => Err(format!("Floti could not write {}: {err}", path.display())),
            };
        }
        self.changed(name, file)
    }

    /// The answer to a change that was made to `file`, once what is kept of
    /// `name` is down to its newest rows.
    fn changed(&self, name: &str, file: &str) -> Result<AgentChange, String> {
        versions::prune(self.conn, &self.key(), name)?;
        Ok(AgentChange {
            changed: true,
            file: self.current(name, file)?,
            refusal: None,
            problems: Vec::new(),
            conflict: None,
        })
    }
}

/// The answer to a change based on a file that is not `file` as it is now.
fn conflicted(name: &str, file: &str, now: &FileState) -> AgentChange {
    let refusal = match now {
        FileState::Absent => format!(
            "{file} was not changed: it is no longer in the agents folder, so it changed since it was read."
        ),
        FileState::Present { .. } => format!(
            "{file} was not changed: it changed since it was read. Read it again and make the change on what is there now."
        ),
    };
    AgentChange {
        changed: false,
        file: agent_file(name, file, now, true),
        refusal: Some(refusal),
        problems: Vec::new(),
        conflict: Some(now.conflict()),
    }
}

/// The answer to a restore whose `file` is not as `version`'s change left
/// it.
fn changed_since(version: &Version, name: &str, file: &str, now: &FileState) -> AgentChange {
    AgentChange {
        changed: false,
        file: agent_file(name, file, now, true),
        refusal: Some(format!(
            "{} was not put back: {file} changed since it was {}, and putting it back would lose that change.",
            version.file,
            version.reason.done()
        )),
        problems: Vec::new(),
        conflict: Some(now.conflict()),
    }
}

fn not_text(file: &str) -> String {
    format!(
        "{file} is not UTF-8 text, so Floti cannot keep it before changing it, and leaves it as it is."
    )
}

/// Writes `text` at `path`, which must not exist, making its folder when it
/// has none.
fn create_file(path: &Path, text: &str) -> io::Result<()> {
    crate::workspace::templates::seed::write_new(path, text)
}

/// Writes `text` over the file at `path`, through a file beside it renamed
/// over it, so the file is never half written.
fn replace_file(path: &Path, text: &str) -> io::Result<()> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let beside = path.with_file_name(format!(".{name}.floti-write"));
    let written = std::fs::File::create(&beside)
        .and_then(|mut out| out.write_all(text.as_bytes()).and_then(|()| out.sync_all()))
        .and_then(|()| std::fs::rename(&beside, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&beside);
    }
    written
}

#[cfg(test)]
mod tests;
