//! The Node the host's tests run the fake agents and their Node scripts on.
//!
//! A bare `node` is whatever the OS finds first, and that is not always a
//! Node: a session inside the installed app has the app's `resources` folder
//! first on `PATH`, which holds a `node` folder that Windows' pty search takes
//! for the program, and a machine can have a stale or broken `node` ahead of
//! the working one. So a test never starts `node` by name. It starts the
//! program [`node`] answers: the first `node` on `PATH` that runs as Node,
//! named by the absolute path Node itself reports.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

static NODE: LazyLock<PathBuf> = LazyLock::new(|| {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let folders: Vec<PathBuf> = std::env::split_paths(&path).collect();
    choose(&folders, runs_as_node).unwrap_or_else(|| {
        panic!(
            "the host's tests run their fakes on Node, and no `node` on PATH runs as Node: {}",
            folders
                .iter()
                .map(|folder| folder.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
});

/// The Node every test runs its fakes and Node scripts on, by absolute path.
/// Found once per test process.
pub fn node() -> &'static Path {
    &NODE
}

/// The file a `node` in `folder` would be on this OS.
fn candidate_in(folder: &Path) -> PathBuf {
    folder.join(format!("node{}", std::env::consts::EXE_SUFFIX))
}

/// The first candidate in `folders`, in order, that `proves` runs as Node,
/// as the path `proves` answers for it.
fn choose(folders: &[PathBuf], proves: impl Fn(&Path) -> Option<PathBuf>) -> Option<PathBuf> {
    folders
        .iter()
        .filter(|folder| folder.is_absolute())
        .map(|folder| candidate_in(folder))
        .find_map(|candidate| proves(&candidate))
}

/// Where `candidate` says its Node is, when it is a file that runs, exits 0
/// and names an existing file as `process.execPath`.
fn runs_as_node(candidate: &Path) -> Option<PathBuf> {
    if !candidate.is_file() {
        return None;
    }
    let output = Command::new(candidate)
        .args(["-e", "process.stdout.write(process.execPath)"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let reported = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    reported.is_file().then_some(reported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders(names: &[&str]) -> Vec<PathBuf> {
        let root = std::env::temp_dir();
        names.iter().map(|name| root.join(name)).collect()
    }

    #[test]
    fn the_first_node_that_runs_as_node_is_chosen_past_one_that_does_not() {
        let path = folders(&["broken", "working", "also working"]);
        let chosen = choose(&path, |candidate| {
            let folder = candidate.parent().unwrap();
            (folder.file_name().unwrap() != "broken").then(|| folder.join("real node"))
        });
        assert_eq!(chosen, Some(path[1].join("real node")));
    }

    #[test]
    fn a_relative_path_entry_is_never_tried() {
        let path = vec![PathBuf::from("relative"), folders(&["working"]).remove(0)];
        let tried = std::sync::Mutex::new(Vec::new());
        let chosen = choose(&path, |candidate| {
            tried.lock().unwrap().push(candidate.to_path_buf());
            Some(candidate.to_path_buf())
        });
        assert_eq!(chosen, Some(candidate_in(&path[1])));
        assert_eq!(*tried.lock().unwrap(), vec![candidate_in(&path[1])]);
    }

    #[test]
    fn none_is_chosen_when_no_candidate_runs_as_node() {
        assert_eq!(choose(&folders(&["a", "b"]), |_| None), None);
    }

    #[test]
    fn a_folder_named_node_or_a_missing_file_is_not_node() {
        let dirs = crate::testkit::TestDirs::new();
        let folder = dirs.folder("holds a folder named node");
        std::fs::create_dir_all(candidate_in(&folder)).unwrap();
        assert_eq!(runs_as_node(&candidate_in(&folder)), None);
        assert_eq!(runs_as_node(&dirs.folder("empty").join("node")), None);
    }

    #[test]
    fn the_node_chosen_runs_and_is_named_by_an_absolute_path() {
        let chosen = node();
        assert!(chosen.is_absolute(), "{}", chosen.display());
        assert_eq!(runs_as_node(chosen).as_deref(), Some(chosen));
    }
}
