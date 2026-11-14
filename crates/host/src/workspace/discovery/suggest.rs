//! Where to point a fresh install. The host only ever suggests: it never scans a folder the user did
//! not choose, and never falls back to the home folder, which would walk a
//! decade of unrelated files on a first run.

use std::path::{Path, PathBuf};

use super::scan::is_workspace;
use crate::agent_config::normalize;

/// The nearest folder at or above `start` that holds a `workspace.yaml`. The
/// host is normally started from inside the workspace it serves, so where it
/// runs is the strongest hint there is.
pub fn find_enclosing_workspace(start: &Path) -> Option<PathBuf> {
    let mut current = normalize(start);
    loop {
        if is_workspace(&current) {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Ordered best guesses for a first-run scan root, each an existing folder
/// and each once ignoring case: the folder holding the workspace enclosing
/// `start` (it covers that workspace and its siblings), then `.harness` in
/// `home`. Empty is an answer: the client then asks rather than guesses.
pub fn suggest_roots(start: &Path, home: Option<&Path>) -> Vec<String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(enclosing) = find_enclosing_workspace(start) {
        candidates.push(
            enclosing
                .parent()
                .map_or_else(|| enclosing.clone(), Path::to_path_buf),
        );
    }
    if let Some(home) = home {
        candidates.push(home.join(".harness"));
    }
    let mut out: Vec<String> = Vec::new();
    for candidate in candidates {
        let normalised = normalize(&candidate).to_string_lossy().into_owned();
        if out
            .iter()
            .any(|existing| existing.to_lowercase() == normalised.to_lowercase())
        {
            continue;
        }
        if std::fs::metadata(&normalised).is_ok_and(|meta| meta.is_dir()) {
            out.push(normalised);
        }
    }
    out
}
