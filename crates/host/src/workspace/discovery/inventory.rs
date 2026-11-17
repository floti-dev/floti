//! Counts what a project holds in either shape a workspace is written in:
//! Claude's (`CLAUDE.md` and `.claude/`) and the shared one (`AGENTS.md` and
//! `.agents/skills`), the way a launch composes them
//! (`agent::workspace::WorkspaceProject`).
//!
//! A *skill* is a folder holding `SKILL.md`, at any depth; *commands* and
//! *agents* are markdown files at any depth, since namespaced commands
//! (`/spec:plan`) live in subfolders. Links are never followed: a junction
//! pointing back up the tree would otherwise recurse forever.

use std::collections::BTreeMap;
use std::path::Path;

use floti_protocol::channels::workspace::ClaudeInventory;

fn exists(path: &Path) -> bool {
    std::fs::metadata(path).is_ok()
}

fn is_directory(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// Each entry of `dir` that is a folder or a file, judged without following
/// links (a link is neither); nothing when it cannot be listed.
fn entries(dir: &Path) -> Vec<(std::path::PathBuf, std::fs::FileType)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(|entry| {
        let entry = entry.ok()?;
        let kind = entry.file_type().ok()?;
        (!kind.is_symlink()).then(|| (entry.path(), kind))
    })
    .collect()
}

fn count_markdown(dir: &Path) -> u32 {
    entries(dir)
        .into_iter()
        .map(|(path, kind)| {
            if kind.is_dir() {
                count_markdown(&path)
            } else if kind.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().to_lowercase().ends_with(".md"))
            {
                1
            } else {
                0
            }
        })
        .sum()
}

fn count_skills(dir: &Path) -> u32 {
    entries(dir)
        .into_iter()
        .filter(|(_, kind)| kind.is_dir())
        .map(|(path, _)| {
            if exists(&path.join("SKILL.md")) {
                1
            } else {
                count_skills(&path)
            }
        })
        .sum()
}

/// The skills in `skills`, by each folder directly in it: one for a folder
/// holding `SKILL.md`, and the skills under it for one that groups them.
fn skills_by_folder(skills: &Path) -> BTreeMap<String, u32> {
    entries(skills)
        .into_iter()
        .filter(|(_, kind)| kind.is_dir())
        .filter_map(|(path, _)| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let count = if exists(&path.join("SKILL.md")) {
                1
            } else {
                count_skills(&path)
            };
            Some((name, count))
        })
        .collect()
}

/// The skills a launch composes from `project`: those in `.claude/skills`,
/// then those in `.agents/skills` whose folder `.claude/skills` has no folder
/// of the same name for, since a launch takes Claude's where both have one.
fn count_both_shapes(project: &Path) -> u32 {
    let claude = skills_by_folder(&project.join(".claude").join("skills"));
    let shared = skills_by_folder(&project.join(".agents").join("skills"));
    let shared_only = shared
        .iter()
        .filter(|(name, _)| !claude.contains_key(*name))
        .map(|(_, count)| count);
    claude.values().chain(shared_only).sum()
}

/// What `project` holds in either shape: its `.claude/`, its skills from
/// `.claude/skills` and `.agents/skills`, and whether it has a `CLAUDE.md`,
/// an `AGENTS.md`, a `.agents/` and a `.mcp.json`. An instructions file or a
/// `.mcp.json` counts with no folder beside it: each alone changes how a
/// session behaves.
pub fn read_inventory(project: &Path) -> ClaudeInventory {
    let claude = project.join(".claude");
    ClaudeInventory {
        skills: count_both_shapes(project),
        commands: count_markdown(&claude.join("commands")),
        agents: count_markdown(&claude.join("agents")),
        hooks: is_directory(&claude.join("hooks")),
        settings: exists(&claude.join("settings.json"))
            || exists(&claude.join("settings.local.json")),
        claude_md: exists(&project.join("CLAUDE.md")),
        mcp: exists(&project.join(".mcp.json")),
        agents_md: exists(&project.join("AGENTS.md")),
        agents_dir: is_directory(&project.join(".agents")),
    }
}

/// Whether `project` has a `.claude/` folder.
pub fn has_claude_dir(project: &Path) -> bool {
    is_directory(&project.join(".claude"))
}
