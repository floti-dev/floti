//! What a first start writes into the templates folder: a README that says
//! how to make a template, and nothing else. The one template Floti ships,
//! Starter, is code (`workspace.rs`), so it reaches people whose templates
//! folder already exists.
//!
//! The trigger is the folder being absent, not a stamp or a hash. The host
//! keeps no record of what it wrote there, so it cannot tell an edited file
//! from an untouched one, and an improved README overwriting somebody's
//! edited copy is the one failure this must not have. So a better shipped
//! README never reaches an existing folder, nothing already there (an example
//! an older Floti seeded included) is ever changed or removed, and deleting
//! the folder brings the README back at the next start.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The shipped files, by path relative to the templates folder.
pub const SHIPPED_TEMPLATES: &[(&str, &str)] = &[("README.md", README)];

const README: &str = r#"# Workspace templates

A template is a folder in this directory. Creating a workspace from it copies the
folder's contents into the new workspace, and that is the whole mechanism.

    templates/
      README.md          <- this file
      client-work/       <- one template, named by its folder
        template.yaml    <- what the picker shows
        ...              <- everything else is copied as-is

## Making one

- **Save a folder as a template.** A workspace's page has Save as template,
  and the template manager (Manage templates in the New workspace dialog or
  in Settings) has Import folder... for any other folder. Both list what will
  be copied, with `repos/` left out unless you tick it, before anything is;
  `.git` and `node_modules` are never copied.
- **Start an empty one.** New, in the template manager, makes a folder here
  with a `template.yaml`, to fill in from there.
- **Or write the folder yourself.** Any folder you put in this directory is a
  template, and the picker reads this directory each time it opens.

## template.yaml

    label: "Client work"
    description: "One sentence, shown under the label in the picker."
    order: 10

All three are optional. Without a label the folder name is used; `order` sorts
the picker, and a template without one sorts after every template with one. A
`template.yaml` that cannot be parsed skips that one template and leaves the
rest of the picker working.

## The rest of the folder

Copied verbatim into the new workspace, with five rules:

- **`.tpl` files are filled in and the extension is dropped.**
  `AGENTS.md.tpl` is written as `AGENTS.md` with `{{NAME}}`,
  `{{CREATED_AT}}`, `{{TEMPLATE}}` and `{{PATH}}` substituted. There
  are no other placeholders, and no conditionals or loops.
- **`{{PATH}}` is the new workspace's folder, with forward slashes** -
  `C:/Users/you/workspaces/work`. Use it wherever a file has to name something
  inside the workspace: a hook command in `settings.json.tpl`, an MCP server in
  `.mcp.json.tpl`. Forward slashes keep it valid JSON, and every tool that
  reads those files accepts them.
- **Every other file is copied byte for byte.** A workflow file full of
  `${{ ... }}` arrives exactly as you wrote it. That is why substitution is
  opt-in rather than applied everywhere.
- **`dot-claude/` is written as `.claude/`.** A literal `.claude/` works
  too; the alias exists because some tools drop dot directories when a template
  is packaged or copied.
- **An empty folder needs a `.gitkeep`.** Folders are created as the parents
  of files, so an empty one would otherwise never arrive. The `.gitkeep`
  itself is not copied.

## What is refused

Absolute paths, anything that climbs out of the workspace with `..`, and
junctions or symlinks. `workspace.yaml` too - Floti writes that one, because
`template:` in it records which template a workspace came from.

## Starter

The first row of the picker is built in and is not a folder here. It writes a
manifest, an empty `repos/` and an `AGENTS.md` with the basics of working in
the workspace, and nothing you put in this directory can change or break it.
A folder here named `starter` (or `minimal`, its old name) is not offered.

## Editing

This file is written once, when this directory does not exist. Nothing here is
ever overwritten, so anything you change stays changed - and deleting the
whole directory puts this README back at the next start.
"#;

/// What seeding did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Seeded {
    /// False when the folder was already there, which is the ordinary case.
    pub seeded: bool,
    /// Written, relative to the folder.
    pub created: Vec<String>,
    /// Why seeding stopped part way, or `None`.
    pub problem: Option<String>,
}

/// Writes the shipped README into `dir`, only when `dir` is absent. The folder itself is made by a call that fails when it exists,
/// and every file is written create-new, so of two hosts starting at once
/// only one seeds, and nothing already there is ever written over.
pub fn seed_templates(dir: &Path) -> Seeded {
    if std::fs::symlink_metadata(dir).is_ok() {
        return Seeded::default();
    }
    if let Some(parent) = dir.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        return failed(Vec::new(), &err);
    }
    match std::fs::create_dir(dir) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => return Seeded::default(),
        Err(err) => return failed(Vec::new(), &err),
    }

    let mut shipped: Vec<&(&str, &str)> = SHIPPED_TEMPLATES.iter().collect();
    shipped.sort_by_key(|(path, _)| *path);
    let mut created = Vec::new();
    for (relative, content) in shipped {
        let destination: PathBuf = dir.join(relative.split('/').collect::<PathBuf>());
        if let Err(err) = write_new(&destination, content) {
            return failed(created, &err);
        }
        created.push((*relative).to_owned());
    }
    Seeded {
        seeded: true,
        created,
        problem: None,
    }
}

/// Writes `content` to `path`, making its folders, and fails when `path`
/// exists. Shared with the agents folder's seeding.
pub(crate) fn write_new(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(content.as_bytes())
}

fn failed(created: Vec<String>, err: &io::Error) -> Seeded {
    Seeded {
        seeded: !created.is_empty(),
        created,
        problem: Some(format!("the templates folder could not be seeded: {err}")),
    }
}

#[cfg(test)]
mod tests;
