//! What the content modules' tests share: a fixture of real folders.

use std::path::Path;

use floti_protocol::channels::workspace::ConfigScopeKind;

use crate::workspace::content::tree::{ContentTree, content_scope, read_content_tree};

pub(super) fn put(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A content scope's fixture: named roots, a scope-root file, a nested
/// repository, a folder of scripts, a folder of bytes and an empty named root.
pub(super) fn fixture(root: &Path) {
    put(
        &root.join("notes").join("alpha.md"),
        &[
            "---",
            "type: journal",
            "date: 2026-08-10",
            "tags: [helm, notes]",
            "---",
            "",
            "# Alpha",
            "",
            "Links to [[beta]] and [[nowhere]].",
            "",
        ]
        .join("\n"),
    );
    put(
        &root.join("notes").join("beta.md"),
        "# Beta\n\nAlpha mentions geofencing here.\n",
    );
    put(
        &root
            .join(".claude")
            .join("skills")
            .join("think")
            .join("SKILL.md"),
        "---\nname: think\n---\n\n# think\n",
    );
    put(&root.join("docs").join("SPEC.md"), "# Spec\n");
    put(&root.join("README.md"), "# Readme\n");
    put(
        &root
            .join("repos")
            .join("other")
            .join("notes")
            .join("hidden.md"),
        "# Hidden\n",
    );
    put(
        &root.join("tools").join("rebuild.py"),
        "# rebuild\nprint(\"ok\")\n",
    );
    put(&root.join("tools").join("notes.png"), "not really a png");
    put(
        &root.join("screenshots").join("one.png"),
        "not really a png",
    );
    std::fs::create_dir_all(root.join("context")).unwrap();
}

pub(super) fn tree_of(root: &Path) -> ContentTree {
    read_content_tree(&content_scope(
        root,
        ConfigScopeKind::Workspace,
        Some("fixture"),
    ))
}
