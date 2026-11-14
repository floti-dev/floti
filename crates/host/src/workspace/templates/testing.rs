//! What the template modules' tests share: a world of a templates folder
//! and a store, and helpers that write, read and list real folders.

use std::fs;
use std::path::{Path, PathBuf};

use floti_protocol::channels::workspace::{
    CreateHarnessRequest, CreateTemplateRequest, HarnessMode,
};

use super::authoring::create_template;
use super::harness::{HarnessCreated, create_harness};
use super::reader::list_templates;
use crate::platform::links;
use crate::store::Store;
use crate::testkit::TestDirs;

/// A folder of its own holding `templates/`, and a store for the writes that
/// snapshot.
pub(super) struct World {
    pub(super) root: PathBuf,
    pub(super) templates: PathBuf,
    pub(super) store: Store,
    /// Last: fields drop in order, and the folder goes only once the store is closed.
    _dirs: TestDirs,
}

impl World {
    pub(super) fn new() -> Self {
        let dirs = TestDirs::new();
        let root = dirs.folder("work area");
        let templates = root.join("templates");
        fs::create_dir_all(&templates).unwrap();
        Self {
            store: Store::open(&dirs.data).unwrap(),
            root,
            templates,
            _dirs: dirs,
        }
    }

    pub(super) fn conn(&self) -> &rusqlite::Connection {
        self.store.connection()
    }

    /// Writes `files` as a template folder. Keys are relative paths.
    pub(super) fn plant(&self, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = self.templates.join(name);
        for (relative, content) in files {
            write(&dir.join(relative), content);
        }
        dir
    }

    pub(super) fn create(&self, name: &str, label: Option<&str>) -> bool {
        create_template(
            self.conn(),
            &self.templates,
            &CreateTemplateRequest {
                name: name.into(),
                label: label.map(str::to_owned),
                description: None,
            },
        )
        .ok
    }

    pub(super) fn harness(&self, name: &str, template: Option<&str>) -> HarnessCreated {
        self.harness_in(&self.root, name, template)
    }

    pub(super) fn harness_in(
        &self,
        dir: &Path,
        name: &str,
        template: Option<&str>,
    ) -> HarnessCreated {
        create_harness(
            &CreateHarnessRequest {
                mode: HarnessMode::New,
                dir: dir.display().to_string(),
                name: Some(name.into()),
                template: template.map(str::to_owned),
            },
            Some(&self.templates),
        )
        .unwrap()
    }

    pub(super) fn listed_ids(&self) -> Vec<String> {
        ids(&self.templates)
    }
}

pub(super) fn ids(dir: &Path) -> Vec<String> {
    list_templates(dir)
        .templates
        .into_iter()
        .map(|choice| choice.id)
        .collect()
}

/// Writes `content` at `path`, making its parents.
pub(super) fn write(path: &Path, content: &str) -> PathBuf {
    write_bytes(path, content.as_bytes())
}

/// Paths here are joined with forward-slashed relative parts, which every OS
/// the host runs on reads as separators.
pub(super) fn write_bytes(path: &Path, content: &[u8]) -> PathBuf {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
    path.to_path_buf()
}

pub(super) fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// Every path under a folder, relative, forward-slashed and sorted. A link
/// is listed and never entered. Knows nothing of templates.
pub(super) fn tree(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, into: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            into.push(rel.clone());
            if !links::is_link(&path) && path.is_dir() {
                walk(&path, &rel, into);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out.sort();
    out
}

pub(super) fn line_matches(text: &str, wanted: &str) -> bool {
    text.lines().any(|line| line == wanted)
}

pub(super) fn manifest_value<'a>(manifest: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}: \"");
    manifest
        .lines()
        .find_map(|line| line.strip_prefix(&prefix)?.strip_suffix('"'))
}

pub(super) fn looks_like_iso(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() > 11
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
}
