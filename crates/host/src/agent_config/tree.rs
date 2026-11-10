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
/// `.mcp.json` beside it.
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
        exists: is_dir(&claude_dir) || is_file(&base.join("CLAUDE.md")),
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
