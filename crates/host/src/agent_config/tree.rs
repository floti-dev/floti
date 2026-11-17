//! Config scopes and their trees, read through the backend's
//! [`ConfigSource`], which lists a credential file without opening it.

use std::path::{Path, PathBuf};

use floti_protocol::channels::workspace::{
    ConfigFile, ConfigFileKind, ConfigScope, ConfigScopeKind, ConfigTree,
};

use super::files::normalize;
use super::time::now_iso;
use crate::agent::AgentBackend;

fn is_dir(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn is_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

/// The user scope: the agent's config folder itself, not a folder holding
/// one.
pub fn user_scope(config_dir: &Path) -> ConfigScope {
    let dir = normalize(config_dir).display().to_string();
    ConfigScope {
        kind: ConfigScopeKind::User,
        path: dir.clone(),
        claude_dir: dir,
        label: "User".to_owned(),
        exists: is_dir(config_dir),
    }
}

/// A project's scope: its `.claude` folder and the `CLAUDE.md` and
/// `.mcp.json` beside it. It exists when the folder holds config in either
/// shape: a `.claude/` or a `CLAUDE.md`, or the shared shape's `.agents/` or
/// `AGENTS.md`.
pub fn project_scope(path: &Path, kind: ConfigScopeKind, label: Option<&str>) -> ConfigScope {
    let base = normalize(path);
    let claude_dir = base.join(".claude");
    ConfigScope {
        kind,
        label: label.map_or_else(
            || {
                base.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| base.display().to_string())
            },
            str::to_owned,
        ),
        exists: is_dir(&claude_dir)
            || is_file(&base.join("CLAUDE.md"))
            || is_dir(&base.join(".agents"))
            || is_file(&base.join("AGENTS.md")),
        path: base.display().to_string(),
        claude_dir: claude_dir.display().to_string(),
    }
}

/// The files in `scope`, as `backend` lists them. A folder that could not be
/// read is an error beside the files, and a backend with no config files to
/// read is one too: the tree is empty because nothing could be looked at,
/// not because nothing is there.
pub fn read_tree(backend: &dyn AgentBackend, scope: &ConfigScope) -> ConfigTree {
    let (files, errors) = match scope_files(backend, scope) {
        Ok(files) => (files, Vec::new()),
        Err(error) => (Vec::new(), vec![error]),
    };
    ConfigTree {
        scope: scope.clone(),
        files,
        errors,
        scanned_at: now_iso(),
    }
}

/// `scope`'s own files. The source lists the user's files and then a
/// project's; a project's are the ones whose path is the project joined to
/// their relative path, which a user file's never is.
pub fn scope_files(
    backend: &dyn AgentBackend,
    scope: &ConfigScope,
) -> Result<Vec<ConfigFile>, String> {
    let base = PathBuf::from(&scope.path);
    let project = (scope.kind != ConfigScopeKind::User).then_some(base.as_path());
    let Some(source) = backend.config() else {
        return Err(format!(
            "{} has no config files Floti can read.",
            backend.name()
        ));
    };
    let listed = source
        .config_files(project)
        .map_err(|err| err.to_string())?;
    Ok(match project {
        None => listed,
        Some(project) => listed
            .into_iter()
            .filter(|file| Path::new(&file.path) == project.join(&file.rel_path))
            .collect(),
    })
}

/// Every file that makes one entry of a scope: a skill's whole folder, its
/// `SKILL.md` first, or the one file for every other kind. Taken from the
/// tree's own listing, so what an import names and what it copies are the
/// same set.
pub fn config_unit<'a>(files: &'a [ConfigFile], file: &'a ConfigFile) -> Vec<&'a ConfigFile> {
    if file.kind != ConfigFileKind::Skill {
        return vec![file];
    }
    let Some(slash) = file.rel_path.rfind('/') else {
        return vec![file];
    };
    let prefix = format!("{}/", file.rel_path[..slash].to_lowercase());
    let mut unit = vec![file];
    unit.extend(files.iter().filter(|candidate| {
        candidate.path != file.path && candidate.rel_path.to_lowercase().starts_with(&prefix)
    }));
    unit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::TestDirs;

    /// Whether a folder holding only `entry` (a folder when it ends in `/`)
    /// is a scope that exists.
    fn exists_with(dirs: &TestDirs, entry: &str) -> bool {
        let folder = dirs.folder(&format!("holds {}", entry.trim_end_matches('/')));
        if let Some(dir) = entry.strip_suffix('/') {
            std::fs::create_dir(folder.join(dir)).unwrap();
        } else if !entry.is_empty() {
            std::fs::write(folder.join(entry), "").unwrap();
        }
        project_scope(&folder, ConfigScopeKind::Project, None).exists
    }

    #[test]
    fn a_folder_with_config_in_either_shape_is_a_scope_that_exists() {
        let dirs = TestDirs::new();
        assert!(!exists_with(&dirs, ""));
        for entry in [".claude/", "CLAUDE.md", ".agents/", "AGENTS.md"] {
            assert!(exists_with(&dirs, entry), "{entry}");
        }
    }

    #[test]
    fn a_scope_in_the_shared_shape_lists_its_agents_md_and_its_agents_skills() {
        let dirs = TestDirs::new();
        let backend = super::super::test_backend(&dirs);
        let folder = dirs.folder("hub");
        let skill = folder.join(".agents").join("skills").join("plan");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(folder.join("AGENTS.md"), "# hub\n").unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\ndescription: Plans.\n---\n").unwrap();
        std::fs::write(skill.join("reference.md"), "# reference\n").unwrap();
        // Not configuration, so not listed.
        std::fs::write(folder.join(".agents").join("notes.md"), "").unwrap();

        let scope = project_scope(&folder, ConfigScopeKind::Project, None);
        let tree = read_tree(&backend, &scope);
        assert!(tree.errors.is_empty(), "{:?}", tree.errors);
        let listed: Vec<(ConfigFileKind, &str, &str)> = tree
            .files
            .iter()
            .map(|file| (file.kind, file.name.as_str(), file.rel_path.as_str()))
            .collect();
        assert_eq!(
            listed,
            [
                (ConfigFileKind::AgentsMd, "AGENTS.md", "AGENTS.md"),
                (
                    ConfigFileKind::Skill,
                    "plan",
                    ".agents/skills/plan/SKILL.md"
                ),
                (
                    ConfigFileKind::Other,
                    "plan/reference.md",
                    ".agents/skills/plan/reference.md"
                ),
            ]
        );
        let plan = &tree.files[1];
        assert_eq!(Path::new(&plan.path), skill.join("SKILL.md"));
        assert_eq!(plan.description.as_deref(), Some("Plans."));
    }
}
