//! What composition reads of an agent's config, and the editors' highlighter:
//! `config:scopes`, `config:tree`, `config:effective` and `editor:highlight`.
//!
//! The scopes and trees are what a template imports from, and the effective
//! view is what a profile composes. None of them writes anything. The file,
//! write and version shapes here are the ones `content:*` carries for a note.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Millis;

request_channel!(ListConfigScopes = "config:scopes", () => Vec<ConfigScope>);
request_channel!(ReadConfigTree = "config:tree", ConfigScopeRef => ConfigTree);
request_channel!(
    /// What a session started here would actually be given.
    ReadEffectiveView = "config:effective", EffectiveViewRequest => EffectiveView
);
request_channel!(
    /// A draft, tokenised per line for an editor's underlay. Opens no file
    /// and writes none.
    HighlightSource = "editor:highlight", SourceToRender => EditorHighlight
);

/// Where a config tree sits in the precedence chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ConfigScopeKind {
    User,
    Harness,
    Project,
}

/// One config tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfigScope {
    pub kind: ConfigScopeKind,
    /// The folder config resolves from; for `user`, the home folder.
    pub path: String,
    /// The `.claude` folder.
    pub claude_dir: String,
    pub label: String,
    pub exists: bool,
}

/// A config scope, by its base folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfigScopeRef {
    pub scope_path: String,
}

/// What a file in a config tree is, as the agent resolves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigFileKind {
    Skill,
    Command,
    Agent,
    Workflow,
    Hook,
    Settings,
    SettingsLocal,
    ClaudeMd,
    Mcp,
    Rule,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    pub path: String,
    /// Relative to the scope's base folder, forward-slashed.
    pub rel_path: String,
    pub kind: ConfigFileKind,
    /// How it is addressed: a skill's folder name, a command's namespace
    /// path, or the file name.
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
    pub mtime_ms: Millis,
    /// `description:` from the frontmatter, when there is one.
    pub description: Option<String>,
    /// A file the console will not offer to edit as text.
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfigTree {
    pub scope: ConfigScope,
    pub files: Vec<ConfigFile>,
    /// Folders that could not be read; the rest of the tree stands.
    pub errors: Vec<String>,
    pub scanned_at: String,
}

/// A file's bytes, with the hash every write is checked against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileContent {
    pub path: String,
    pub exists: bool,
    pub content: String,
    /// sha256 of the bytes on disk, hex.
    pub hash: String,
    #[ts(type = "number")]
    pub size: u64,
    pub mtime_ms: Millis,
    /// The bytes are not text; an editor refuses these.
    pub binary: bool,
}

/// Why a snapshot was taken. A note's versions are taken for an edit or a
/// restore; the other reasons are those of versions an agent's config files
/// were given by earlier releases, which the store still holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ConfigWriteReason {
    Edit,
    Create,
    Restore,
    Mcp,
    Approve,
    Rename,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSnapshotMeta {
    #[ts(type = "number")]
    pub id: u64,
    pub scope_path: String,
    /// Relative to `scopePath`, forward-slashed.
    pub file_path: String,
    pub content_hash: String,
    #[ts(type = "number")]
    pub bytes: u64,
    pub reason: ConfigWriteReason,
    pub created_at: String,
}

/// One file in a scope: whose versions to list, or which file to open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScopedFile {
    pub scope_path: String,
    pub path: String,
}

/// Puts a snapshot's bytes back at `path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSnapshotRequest {
    #[ts(type = "number")]
    pub id: u64,
    pub path: String,
}

/// Text to render or highlight as the file at `path` would be. The text is
/// what is in the editor, which may not be on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceToRender {
    pub path: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WriteConfigRequest {
    /// The scope's base folder, recorded on the snapshot.
    pub scope_path: String,
    pub path: String,
    pub content: String,
    /// The hash the editor's content came from, or null for a file it knows
    /// does not exist yet. A mismatch stops the write.
    pub expected_hash: Option<String>,
    pub reason: ConfigWriteReason,
}

/// The file on disk is not what the editor was based on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WriteConflict {
    pub on_disk_hash: String,
    pub on_disk_content: String,
    pub mtime_ms: Millis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WriteConfigResult {
    pub ok: bool,
    /// The file after the write, or as it stands when the write was refused.
    pub file: ConfigFileContent,
    /// The snapshot taken first, or null when nothing was written.
    #[ts(type = "number | null")]
    pub snapshot_id: Option<u64>,
    /// The bytes were already what was asked for.
    pub unchanged: bool,
    /// Set when the file on disk is not what the editor was based on.
    #[serde(default)]
    pub conflict: Option<WriteConflict>,
    /// Set when the write was refused for another reason.
    #[serde(default)]
    pub error: Option<String>,
}

/// Where a resolved entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum EffectiveSource {
    User,
    Cwd,
    Overlay,
}

/// One skill, command, agent or workflow as a session would address it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveEntry {
    /// What you type: `acme:think`, or `think` unprefixed.
    pub invocation: String,
    pub name: String,
    pub source: EffectiveSource,
    /// The overlay's namespace, or null when it resolves unprefixed.
    pub namespace: Option<String>,
    /// The folder it came from.
    pub origin: String,
    pub path: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SettingsLayerKind {
    User,
    Project,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingsLayer {
    pub kind: SettingsLayerKind,
    pub file: String,
    pub exists: bool,
    /// Leaf paths the layer defines.
    pub keys: u32,
    /// Set when the file is there but could not be parsed.
    pub error: Option<String>,
}

/// One layer's value for a setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingCandidate {
    pub layer: SettingsLayerKind,
    pub file: String,
    /// JSON encoding of the value.
    pub value: String,
}

/// One setting leaf, and which layer's value a session would see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveSetting {
    /// The leaf path, such as `env.FOO`.
    pub key: String,
    /// JSON encoding of the winning value.
    pub value: String,
    pub winner: SettingsLayerKind,
    pub winner_file: String,
    /// Every layer defining this key, highest precedence first.
    pub candidates: Vec<SettingCandidate>,
    /// A lower layer's value is shadowed.
    pub overridden: bool,
}

/// An MCP server as configured, and whether a session would load it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveMcpServer {
    pub name: String,
    pub scope: McpScope,
    /// The file that defines it.
    pub file: String,
    /// The server's JSON, pretty-printed.
    pub config: String,
    pub transport: String,
    /// Null for scopes where approval does not arise.
    pub approved: Option<bool>,
    pub approved_by: Option<String>,
    /// Set when a higher-precedence scope defines the same name.
    pub shadowed_by: Option<String>,
}

/// An overlay a session would be given, and what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveOverlay {
    /// The plugin manifest name, which is the namespace.
    pub name: String,
    pub project_path: String,
    pub exists: bool,
    pub skills: u32,
    pub commands: u32,
    pub agents: u32,
    pub workflows: u32,
}

/// A name carried by more than one source, each under its own invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SharedName {
    pub name: String,
    pub invocations: Vec<String>,
}

/// An instruction file a session would be given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveInstruction {
    pub path: String,
    pub source: EffectiveSource,
    #[ts(type = "number")]
    pub bytes: u64,
    pub origin: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveViewRequest {
    /// Computed for a saved profile: its root and its overlays.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub profile_id: Option<u64>,
    /// Or for a plain folder, with the overlays passed alongside.
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub overlays: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveView {
    pub cwd: String,
    /// The profile this was computed for, or null for a plain folder.
    #[ts(type = "number | null")]
    pub profile_id: Option<u64>,
    pub profile_name: Option<String>,
    pub overlays: Vec<EffectiveOverlay>,
    pub skills: Vec<EffectiveEntry>,
    pub commands: Vec<EffectiveEntry>,
    pub agents: Vec<EffectiveEntry>,
    pub workflows: Vec<EffectiveEntry>,
    pub shared_names: Vec<SharedName>,
    pub settings_layers: Vec<SettingsLayer>,
    pub settings: Vec<EffectiveSetting>,
    /// Instruction files, in the order they arrive.
    pub instructions: Vec<EffectiveInstruction>,
    pub mcp_servers: Vec<EffectiveMcpServer>,
    pub warnings: Vec<String>,
    pub computed_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum McpScope {
    Local,
    User,
    Project,
}

/// A draft, tokenised per line for an editor's underlay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct EditorHighlight {
    /// The inner HTML of each line; empty when `tooLarge`.
    pub lines: Vec<String>,
    pub language: String,
    pub highlighted: bool,
    pub too_large: bool,
    pub took_ms: Millis,
}
