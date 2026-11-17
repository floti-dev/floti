//! Making a workspace, as opposed to finding one.
//!
//! A workspace is a folder with a `workspace.yaml` in it, or the
//! `harness.yaml` it had before; a new one always gets `workspace.yaml`, and
//! neither shape is made over a folder that has either. Two shapes:
//!
//! - `new`: a folder that does not exist yet. From Starter it gets
//!   `workspace.yaml`, an empty `repos/` and an `AGENTS.md` naming it, and
//!   nothing else; from one of the user's templates, that template's tree and
//!   a manifest.
//! - `convert`: a folder that already holds repositories. It gets a
//!   `workspace.yaml`, with `repos: "."` when it has no `repos/` so the
//!   repositories at its top level stay visible, and nothing else: its own
//!   instructions stay as they are. Templates do not apply: converting is
//!   about a folder somebody already has, and writing a layout into it would
//!   be writing into their work.

use std::path::{Path, PathBuf};

use floti_protocol::channels::workspace::{CreateWorkspaceRequest, WorkspaceMode};

use super::reader::{TemplateValues, apply_template, template_id_problems, template_path_value};
use super::{
    STARTER_TEMPLATE, WORKSPACE_MANIFEST, WORKSPACE_MANIFESTS, exists, is_built_in, is_real_dir,
};
use crate::agent_config::normalize;
use crate::agent_config::time::now_iso;

/// The manifest format this writer produces; not the host's version, nor
/// the workspace's.
pub const WORKSPACE_FORMAT_VERSION: &str = "1";

/// The instructions Starter writes, with `{{NAME}}` for the workspace's name.
/// `AGENTS.md` is the shape every full backend reads.
const STARTER_AGENTS_MD: &str = include_str!("starter/AGENTS.md");

/// Starter's `AGENTS.md` for a workspace called `name`.
fn starter_agents_md(name: &str) -> String {
    STARTER_AGENTS_MD.replace("{{NAME}}", name)
}

/// What making a workspace did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceCreated {
    /// The workspace folder, or `None` when nothing was written.
    pub path: Option<PathBuf>,
    /// Written, relative to `path`, in the order written.
    pub created: Vec<String>,
    /// With `path` `None`, why nothing was written; with it set, the
    /// template entries that did not land. No rollback is claimed.
    pub problems: Vec<String>,
}

impl WorkspaceCreated {
    fn refused(problems: Vec<String>) -> Self {
        Self {
            problems,
            ..Self::default()
        }
    }
}

/// Reserved on Windows whatever the extension; refused on every OS, since a
/// workspace is a folder people copy between machines.
fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().to_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ((stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

/// Problems with a proposed folder name, as sentences. Spaces and dashes
/// are ordinary in a folder name; these are the ones Windows refuses.
pub fn workspace_name_problems(name: &str) -> Vec<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return vec!["A name is required.".to_owned()];
    }
    let mut problems = Vec::new();
    if trimmed == "." || trimmed == ".." {
        problems.push(format!("\"{trimmed}\" is not a folder name."));
    }
    if trimmed.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']) {
        problems.push("A name cannot contain \\ / : * ? \" < > | or a path separator.".to_owned());
    }
    if is_reserved(trimmed) {
        problems.push(format!(
            "\"{trimmed}\" is a reserved device name on Windows."
        ));
    }
    // Windows strips both, so `dev.` would be made as `dev` and every path
    // built from the name would be wrong.
    if trimmed.ends_with('.') || trimmed.ends_with(' ') {
        problems.push("A name cannot end with a space or a dot.".to_owned());
    }
    problems
}

/// The manifest, written by hand, with no comments: `name`, `template`,
/// `version` and `created` for a new workspace; a converted one has no
/// `template` and may have `repos`. A JSON string is a valid YAML
/// double-quoted scalar, so JSON is the escaping.
fn manifest(name: &str, template: Option<&str>, created: &str, repos: Option<&str>) -> String {
    let quoted = |text: &str| serde_json::to_string(text).unwrap_or_default();
    let mut lines = vec![format!("name: {}", quoted(name))];
    if let Some(template) = template {
        lines.push(format!("template: {}", quoted(template)));
    }
    lines.extend([
        format!("version: {}", quoted(WORKSPACE_FORMAT_VERSION)),
        format!("created: {}", quoted(created)),
    ]);
    if let Some(repos) = repos {
        lines.push(format!("repos: {}", quoted(repos)));
    }
    format!("{}\n", lines.join("\n"))
}

/// Makes a workspace as `request` asks, from `templates_dir` when it names a
/// template. A refusal writes nothing; a failure to write once the folder is
/// made is the `Err` sentence.
pub fn create_workspace(
    request: &CreateWorkspaceRequest,
    templates_dir: Option<&Path>,
) -> Result<WorkspaceCreated, String> {
    let dir = normalize(Path::new(&request.dir));
    match request.mode {
        WorkspaceMode::New => create_new(request, &dir, templates_dir),
        WorkspaceMode::Convert => convert(request, &dir),
    }
}

fn create_new(
    request: &CreateWorkspaceRequest,
    dir: &Path,
    templates_dir: Option<&Path>,
) -> Result<WorkspaceCreated, String> {
    let name = request
        .name
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_owned();
    // Absent, empty or either id the built-in has had is Starter, and is
    // recorded as `starter` whichever id asked for it.
    let template = request
        .template
        .as_deref()
        .map(str::trim)
        .filter(|template| !template.is_empty() && !is_built_in(template))
        .unwrap_or(STARTER_TEMPLATE)
        .to_owned();
    let starter = template == STARTER_TEMPLATE;
    let mut problems = workspace_name_problems(&name);
    if !is_real_dir(dir) {
        problems.push(format!("{} is not a folder.", dir.display()));
    }
    if !starter {
        let id_problems = template_id_problems(&template);
        let valid = id_problems.is_empty();
        problems.extend(id_problems);
        match templates_dir {
            None => problems.push("Templates are not available in this run.".to_owned()),
            // Asked before the folder is made: a template that is not there
            // must leave nothing behind.
            Some(templates) if valid && !is_real_dir(&templates.join(&template)) => {
                problems.push(format!("There is no template called \"{template}\"."));
            }
            Some(_) => {}
        }
    }
    if !problems.is_empty() {
        return Ok(WorkspaceCreated::refused(problems));
    }

    let target = dir.join(&name);
    if exists(&target) {
        // Neither an overwrite nor a merge: the caller asked to create one.
        let why = if has_manifest(&target) {
            format!("{} is already a workspace.", target.display())
        } else {
            format!("{} already exists.", target.display())
        };
        return Ok(WorkspaceCreated::refused(vec![why]));
    }

    let created_at = now_iso();
    make_dir(&target)?;
    let mut created = Vec::new();
    let mut problems = Vec::new();
    if starter {
        make_dir(&target.join("repos"))?;
        created.push("repos".to_owned());
        write(&target.join("AGENTS.md"), &starter_agents_md(&name))?;
        created.push("AGENTS.md".to_owned());
    } else {
        // The template's tree first and the manifest last, so `template:`
        // is what the host wrote and a partial apply still leaves a workspace
        // the launcher sees.
        let applied = apply_template(
            templates_dir.unwrap_or(dir),
            &template,
            &target,
            &TemplateValues {
                name: name.clone(),
                created_at: created_at.clone(),
                template: template.clone(),
                path: template_path_value(&target),
            },
        );
        created.extend(applied.created);
        problems.extend(applied.problems);
    }
    write(
        &target.join(WORKSPACE_MANIFEST),
        &manifest(&name, Some(&template), &created_at, None),
    )?;
    created.push(WORKSPACE_MANIFEST.to_owned());
    Ok(WorkspaceCreated {
        path: Some(target),
        created,
        problems,
    })
}

/// Whether `dir` is a workspace already, by either name its manifest has had.
fn has_manifest(dir: &Path) -> bool {
    WORKSPACE_MANIFESTS
        .iter()
        .any(|manifest| exists(&dir.join(manifest)))
}

fn convert(request: &CreateWorkspaceRequest, dir: &Path) -> Result<WorkspaceCreated, String> {
    if !is_real_dir(dir) {
        return Ok(WorkspaceCreated::refused(vec![format!(
            "{} is not a folder.",
            dir.display()
        )]));
    }
    if has_manifest(dir) {
        return Ok(WorkspaceCreated::refused(vec![format!(
            "{} is already a workspace.",
            dir.display()
        )]));
    }
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            dir.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    // A folder that already has `repos/` means what the default means.
    let repos = (!is_real_dir(&dir.join("repos"))).then_some(".");
    // No `template:`: a converted folder was never built from a template,
    // whatever the caller passed.
    write(
        &dir.join(WORKSPACE_MANIFEST),
        &manifest(&name, None, &now_iso(), repos),
    )?;
    Ok(WorkspaceCreated {
        path: Some(dir.to_path_buf()),
        created: vec![WORKSPACE_MANIFEST.to_owned()],
        problems: Vec::new(),
    })
}

fn make_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|err| format!("Floti could not create {}: {err}", path.display()))
}

fn write(path: &Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content)
        .map_err(|err| format!("Floti could not write {}: {err}", path.display()))
}

#[cfg(test)]
mod tests;
