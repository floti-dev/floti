//! An agent file read: YAML front matter between `---` lines, then the body.
//!
//! The front matter is split as a note's is (`workspace::render::frontmatter`)
//! and parsed with `serde_yaml_ng`. **A file that is not exactly an agent is
//! bad, never half an agent**: a front matter that does not parse, a key no
//! agent file has and a value outside its field's choices each make the whole file bad,
//! with every problem named, because a typo such as `disalowed_tools` must
//! never quietly drop a rule.
//!
//! The one exception is a retired field ([`RETIRED_FIELDS`]): a line Floti
//! once read and no longer does. The shipped agents wrote such lines into
//! folders that are never rewritten, so a retired line is accepted with any
//! value, ignored, and kept on the agent for the listing to name.
//!
//! Every problem names the field and the line of the file it concerns
//! ([`DefinitionProblem`]), and every value is also read as the file writes
//! it ([`WrittenValue`]), whether or not the file parses, so a client can
//! show each problem and each value on its field.

use std::collections::BTreeMap;

use floti_protocol::BackendId;
use floti_protocol::channels::common::PermissionMode;
use floti_protocol::channels::sessions::EffortLevel;
use serde_yaml_ng::{Mapping, Value};

use super::AgentDefinition;
use crate::agent::input::key_bytes;
use crate::workspace::render::frontmatter::parse_note_frontmatter;

/// The fields an agent file has.
pub const FIELDS: &[&str] = &[
    "description",
    "profile",
    "backend",
    "model",
    "effort",
    "permission",
    "worktree",
    "can_start",
    "allowed_tools",
    "disallowed_tools",
    "command",
    "submit",
    "stop",
];

/// The fields an agent file may still have that Floti no longer reads.
pub const RETIRED_FIELDS: &[&str] = &["class"];

/// The efforts an agent may give.
const EFFORTS: &str = "low, medium, high, xhigh or max";

/// The permission modes an agent may ask for.
const PERMISSIONS: &str = "plan, dont-ask, manual, accept-edits, auto or bypass-permissions";

/// One problem with an agent file: what is wrong, and the field and the line
/// of the file it concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionProblem {
    /// The field, as the file spells it; `None` for a problem with the whole
    /// file.
    pub field: Option<String>,
    /// The file's line, from 1; `None` when the problem is with no one line.
    pub line: Option<u32>,
    /// What is wrong, as a sentence fragment.
    pub message: String,
    /// The backend id of the entry it concerns, for a field keyed by
    /// backend (`model`, `effort`); `None` otherwise.
    pub backend: Option<String>,
}

impl DefinitionProblem {
    /// A problem with the whole file and no one line of it, such as a file
    /// that could not be read.
    pub fn whole(message: impl Into<String>) -> Self {
        Self {
            field: None,
            line: None,
            message: message.into(),
            backend: None,
        }
    }

    fn at(line: u32, message: impl Into<String>) -> Self {
        Self {
            field: None,
            line: Some(line),
            message: message.into(),
            backend: None,
        }
    }
}

/// `problems` as one sentence fragment, in order.
pub fn joined(problems: &[DefinitionProblem]) -> String {
    problems
        .iter()
        .map(|problem| problem.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

/// One field's value as the file writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenValue {
    /// The field, as the file spells it.
    pub field: String,
    /// The line it is on, from 1.
    pub line: u32,
    /// What follows its colon, and the indented lines under it with their
    /// common indent taken off, trimmed.
    pub value: String,
}

/// An agent file read whole: the agent it is or every problem with it, each
/// value as written, and the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFileReading {
    pub agent: Result<AgentDefinition, Vec<DefinitionProblem>>,
    /// Each top-level field of the front matter as written, in the file's
    /// order; empty when there is no front matter.
    pub values: Vec<WrittenValue>,
    /// The text after the front matter, as written; the whole text when
    /// there is no front matter.
    pub body: String,
}

impl AgentFileReading {
    /// The line the field `field` is on, when the file writes it.
    pub fn line_of(&self, field: &str) -> Option<u32> {
        line_of(&self.values, field)
    }
}

fn line_of(values: &[WrittenValue], field: &str) -> Option<u32> {
    values
        .iter()
        .find(|value| value.field == field)
        .map(|value| value.line)
}

/// The agent `name` that `source` (the text of `<name>.md`) describes, or every
/// problem with it, joined, as a sentence fragment.
///
/// # Errors
///
/// What is wrong with the file, when anything is.
pub fn parse_definition(name: &str, source: &str) -> Result<AgentDefinition, String> {
    parse_agent(name, source).map_err(|problems| joined(&problems))
}

/// The agent `name` that `source` describes, or every problem with it, each
/// naming its field and line.
///
/// # Errors
///
/// Every problem with the file, when there is one.
pub fn parse_agent(name: &str, source: &str) -> Result<AgentDefinition, Vec<DefinitionProblem>> {
    read_agent_file(name, source).agent
}

/// `source`, the text of `<name>.md`, read whole.
pub fn read_agent_file(name: &str, source: &str) -> AgentFileReading {
    let front = parse_note_frontmatter(source);
    if !front.present {
        let text = source.strip_prefix('\u{feff}').unwrap_or(source);
        let problem = if text.trim_start().starts_with("---") {
            let fence = text
                .split('\n')
                .position(|line| line.trim().starts_with("---"))
                .unwrap_or(0);
            DefinitionProblem::at(
                line_number(fence),
                "its front matter must start on the file's first line with --- and end with a --- line",
            )
        } else {
            DefinitionProblem::at(
                1,
                "it has no front matter: an agent file starts with --- lines around its fields",
            )
        };
        return AgentFileReading {
            agent: Err(vec![problem]),
            values: Vec::new(),
            body: front.body,
        };
    }
    // The front matter's first line is the file's second, after the fence.
    let values = written_values(&front.raw, 2);
    let body = front.body.clone();
    if let Some(error) = front.error {
        // The parser counts lines from the front matter's first, which is
        // the file's second.
        let line = serde_yaml_ng::from_str::<Value>(&front.raw)
            .err()
            .and_then(|err| err.location())
            .map_or(2, |at| line_number(at.line()));
        return AgentFileReading {
            agent: Err(vec![DefinitionProblem::at(
                line,
                format!("its front matter does not parse: {error}"),
            )]),
            values,
            body,
        };
    }
    let fields = front.data.unwrap_or_default();

    let mut agent = AgentDefinition::new(name, []);
    agent.instructions = front.body.trim().to_owned();
    let mut problems: Vec<DefinitionProblem> = Vec::new();
    for (key, value) in &fields {
        let Some(key) = key.as_str() else {
            let spelled = serde_yaml_ng::to_string(key)
                .map(|text| text.trim().to_owned())
                .unwrap_or_default();
            problems.push(DefinitionProblem {
                line: line_of(&values, &spelled),
                field: Some(spelled).filter(|spelled| !spelled.is_empty()),
                message: "a key that is not text".to_owned(),
                backend: None,
            });
            continue;
        };
        if let Err(FieldError { message, backend }) = apply(&mut agent, key, value) {
            problems.push(DefinitionProblem {
                field: Some(key.to_owned()),
                line: line_of(&values, key),
                message,
                backend,
            });
        }
    }
    AgentFileReading {
        agent: if problems.is_empty() {
            Ok(agent)
        } else {
            Err(problems)
        },
        values,
        body,
    }
}

/// The 1-based line of a 0-based line index.
fn line_number(at: usize) -> u32 {
    u32::try_from(at.saturating_add(1)).unwrap_or(u32::MAX)
}

/// Each top-level field in `raw`, the front matter whose first line is the
/// file's line `first`, as written. A field is a line that starts in the
/// first column with a key and a colon; the lines under it that are
/// indented, blank or a `-` item are its value, up to the next field. A
/// comment in the first column belongs to no field.
fn written_values(raw: &str, first: u32) -> Vec<WrittenValue> {
    let mut values: Vec<(WrittenValue, Vec<&str>)> = Vec::new();
    for (at, line) in raw.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let number = first.saturating_add(u32::try_from(at).unwrap_or(u32::MAX));
        if let Some((field, rest)) = field_line(line) {
            values.push((
                WrittenValue {
                    field,
                    line: number,
                    value: rest.trim().to_owned(),
                },
                Vec::new(),
            ));
        } else if !line.starts_with('#')
            && let Some((_, under)) = values.last_mut()
        {
            under.push(line);
        }
    }
    values
        .into_iter()
        .map(|(mut value, under)| {
            let under = dedented(&under);
            if !under.is_empty() {
                value.value = if value.value.is_empty() {
                    under
                } else {
                    format!("{}\n{under}", value.value)
                };
            }
            value
        })
        .collect()
}

/// The key and what follows its colon, when `line` starts a top-level field.
fn field_line(line: &str) -> Option<(String, &str)> {
    let first = line.chars().next()?;
    if first.is_whitespace() || matches!(first, '#' | '-' | '[' | '{' | '?') {
        return None;
    }
    let (key, rest) = if matches!(first, '"' | '\'') {
        let close = line[1..].find(first)? + 1;
        let rest = line[close + 1..].trim_start().strip_prefix(':')?;
        (line[1..close].to_owned(), rest)
    } else {
        let colon = line.find(':')?;
        (line[..colon].trim_end().to_owned(), &line[colon + 1..])
    };
    (!key.is_empty()).then_some((key, rest))
}

/// `lines` with their common indent taken off, without the blank lines at
/// either end.
fn dedented(lines: &[&str]) -> String {
    let start = lines.iter().position(|line| !line.trim().is_empty());
    let end = lines.iter().rposition(|line| !line.trim().is_empty());
    let (Some(start), Some(end)) = (start, end) else {
        return String::new();
    };
    let kept = &lines[start..=end];
    let indent = kept
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    kept.iter()
        .map(|line| line.get(indent..).unwrap_or("").trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Why a field's value was refused, and the backend whose entry it was for a
/// field keyed by backend.
struct FieldError {
    message: String,
    backend: Option<String>,
}

impl From<String> for FieldError {
    fn from(message: String) -> Self {
        Self {
            message,
            backend: None,
        }
    }
}

impl FieldError {
    fn of_backend(backend: &str, message: String) -> Self {
        Self {
            message,
            backend: Some(backend.to_owned()),
        }
    }
}

/// Sets the field `key` of `agent` from `value`.
fn apply(agent: &mut AgentDefinition, key: &str, value: &Value) -> Result<(), FieldError> {
    match key {
        "description" => agent.description = text(key, value)?.unwrap_or_default(),
        "profile" => agent.profile = text(key, value)?,
        "backend" => agent.backends = backends(value)?,
        "model" => agent.models = keyed_text(key, value)?,
        "effort" => agent.efforts = efforts(value)?,
        "permission" => agent.permission = permission(value)?,
        "worktree" => agent.worktree = flag(key, value)?,
        "can_start" => agent.can_start = flag(key, value)?,
        "allowed_tools" => agent.allowed_tools = list(key, value)?,
        "disallowed_tools" => agent.disallowed_tools = list(key, value)?,
        "command" => agent.command = text(key, value)?,
        "submit" => agent.submit = key_word(key, value)?,
        "stop" => agent.stop = key_word(key, value)?,
        retired if RETIRED_FIELDS.contains(&retired) => {
            agent.unused_lines.push(retired.to_owned());
        }
        other => {
            return Err(FieldError::from(format!(
                "{other} is not a field an agent file has (the fields are {})",
                FIELDS.join(", ")
            )));
        }
    }
    Ok(())
}

/// A text field: nothing for a null or blank value.
fn text(key: &str, value: &Value) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.trim().to_owned()).filter(|text| !text.is_empty())),
        _ => Err(format!("{key} must be text")),
    }
}

/// A text field naming a key (`enter`, `ctrl-c`): the word must be one
/// ([`key_bytes`]), so a typo is a problem with the file and never a key that
/// quietly presses nothing.
fn key_word(key: &str, value: &Value) -> Result<Option<String>, String> {
    let word = text(key, value)?;
    if let Some(word) = &word {
        key_bytes(word).map_err(|problem| format!("{key}: {problem}"))?;
    }
    Ok(word)
}

/// `yes`, `no`, `true` or `false`.
fn flag(key: &str, value: &Value) -> Result<bool, String> {
    match value {
        Value::Null => Ok(false),
        Value::Bool(flag) => Ok(*flag),
        Value::String(word) => match word.trim().to_ascii_lowercase().as_str() {
            "yes" | "true" => Ok(true),
            "no" | "false" => Ok(false),
            _ => Err(format!("{key} must be yes, no, true or false, not {word}")),
        },
        _ => Err(format!("{key} must be yes, no, true or false")),
    }
}

/// A comma list or a YAML list.
fn list(key: &str, value: &Value) -> Result<Vec<String>, String> {
    let items: Vec<String> = match value {
        Value::Null => return Ok(Vec::new()),
        Value::String(text) => split_commas(text),
        Value::Sequence(items) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => Ok(text.trim().to_owned()),
                _ => Err(format!("{key} must list text")),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(format!("{key} must be a comma list or a list")),
    };
    Ok(items.into_iter().filter(|item| !item.is_empty()).collect())
}

/// `text` split at the commas that are not inside parentheses, so that a
/// rule such as `Bash(git log --format=a,b:*)` stays whole.
fn split_commas(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0_u32;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                items.push(current.trim().to_owned());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    items.push(current.trim().to_owned());
    items
}

/// One backend id or several, in order, as a comma list or a YAML list. An id
/// the host does not know is valid: it is simply not installed.
fn backends(value: &Value) -> Result<Vec<BackendId>, String> {
    let ids = list("backend", value)?;
    if matches!(value, Value::Sequence(_) | Value::String(_)) && ids.is_empty() {
        return Err(
            "backend lists no backend; leave the line out for every installed agent".into(),
        );
    }
    let mut out: Vec<BackendId> = Vec::new();
    for id in ids {
        if id.chars().any(char::is_whitespace) {
            return Err(format!("backend {id} is not a backend id"));
        }
        let id = id.to_ascii_lowercase();
        if !out.iter().any(|seen| seen.as_str() == id) {
            out.push(BackendId::new(id));
        }
    }
    Ok(out)
}

/// A map from a backend id to a text.
fn keyed_text(key: &str, value: &Value) -> Result<BTreeMap<String, String>, FieldError> {
    let map = keyed(key, value)?;
    let mut out = BTreeMap::new();
    for (backend, item) in map {
        match item {
            Value::String(text) if !text.trim().is_empty() => {
                out.insert(backend, text.trim().to_owned());
            }
            _ => {
                let message = format!("{key} for {backend} must be text");
                return Err(FieldError::of_backend(&backend, message));
            }
        }
    }
    Ok(out)
}

/// A map from a backend id to an effort.
fn efforts(value: &Value) -> Result<BTreeMap<String, EffortLevel>, FieldError> {
    let mut out = BTreeMap::new();
    for (backend, item) in keyed("effort", value)? {
        let level = item
            .as_str()
            .and_then(|word| match word.trim().to_ascii_lowercase().as_str() {
                "low" => Some(EffortLevel::Low),
                "medium" => Some(EffortLevel::Medium),
                "high" => Some(EffortLevel::High),
                "xhigh" => Some(EffortLevel::Xhigh),
                "max" => Some(EffortLevel::Max),
                _ => None,
            })
            .ok_or_else(|| {
                FieldError::of_backend(&backend, format!("effort for {backend} must be {EFFORTS}"))
            })?;
        out.insert(backend, level);
    }
    Ok(out)
}

/// A mapping's entries, keyed by a backend id (in lowercase).
fn keyed(key: &str, value: &Value) -> Result<Vec<(String, Value)>, String> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Mapping(map) => entries(key, map),
        _ => Err(format!(
            "{key} is a map from a backend id to a value, such as claude-code: opus"
        )),
    }
}

fn entries(key: &str, map: &Mapping) -> Result<Vec<(String, Value)>, String> {
    map.iter()
        .map(|(backend, value)| match backend.as_str() {
            Some(backend) if !backend.trim().is_empty() => {
                Ok((backend.trim().to_ascii_lowercase(), value.clone()))
            }
            _ => Err(format!("{key} must be keyed by backend ids")),
        })
        .collect()
}

/// A permission mode, written with hyphens or spaces or in camelCase.
fn permission(value: &Value) -> Result<Option<PermissionMode>, String> {
    let word = match value {
        Value::Null => return Ok(None),
        Value::String(word) => word,
        _ => return Err(format!("permission must be {PERMISSIONS}")),
    };
    let squashed: String = word
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_' | '\''))
        .collect::<String>()
        .to_ascii_lowercase();
    match squashed.as_str() {
        "plan" => Ok(Some(PermissionMode::Plan)),
        "dontask" => Ok(Some(PermissionMode::DontAsk)),
        "manual" => Ok(Some(PermissionMode::Manual)),
        "acceptedits" => Ok(Some(PermissionMode::AcceptEdits)),
        "auto" => Ok(Some(PermissionMode::Auto)),
        "bypasspermissions" => Ok(Some(PermissionMode::BypassPermissions)),
        _ => Err(format!("permission must be {PERMISSIONS}, not {word}")),
    }
}
