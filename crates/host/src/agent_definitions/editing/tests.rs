//! An agents folder read and changed as a client changes it: what a write,
//! a delete, a rename, a restore and an add each do to the files and to what
//! is kept of them, and what each refuses. A real folder and a real floti.db
//! under the test's run root.

use std::path::{Path, PathBuf};

use floti_protocol::channels::agents::{
    AgentChange, AgentProblem, AgentWrittenValue, DeleteAgentRequest, WriteAgentRequest,
};
use floti_protocol::channels::common::PermissionMode;

use super::*;
use crate::agent_definitions::versions::{self, KEPT_PER_AGENT};
use crate::agent_definitions::{DefinitionLookup, DefinitionSource};
use crate::store::Store;
use crate::testkit::{TestDirs, link_file};

const REVIEWER: &str = "---\ndescription: Reads a change\npermission: plan\n---\nRead the diff.\n";
const REVIEWER_EDITED: &str =
    "---\ndescription: Reads a change closely\npermission: plan\n---\nRead every line.\n";

struct World {
    folder: FolderDefinitions,
    store: Store,
    dirs: TestDirs,
}

impl World {
    fn new() -> Self {
        let dirs = TestDirs::new();
        Self {
            folder: FolderDefinitions::new(dirs.config.join("agents")),
            store: Store::open(&dirs.data).unwrap(),
            dirs,
        }
    }

    fn editor(&self) -> AgentEditor<'_> {
        AgentEditor::new(&self.folder, self.store.connection(), false)
    }

    fn read_only(&self) -> AgentEditor<'_> {
        AgentEditor::new(&self.folder, self.store.connection(), true)
    }

    fn dir(&self) -> &Path {
        self.folder.dir()
    }

    fn path(&self, file: &str) -> PathBuf {
        self.dir().join(file)
    }

    /// Writes `file` as an outside editor would.
    fn put(&self, file: &str, text: &str) {
        std::fs::create_dir_all(self.dir()).unwrap();
        std::fs::write(self.path(file), text).unwrap();
    }

    fn text(&self, file: &str) -> Option<String> {
        std::fs::read_to_string(self.path(file)).ok()
    }

    fn kept(&self, name: &str) -> Vec<versions::Version> {
        versions::all(
            self.store.connection(),
            &self.dir().display().to_string(),
            name,
        )
        .unwrap()
    }

    fn write(&self, name: &str, text: &str, expected: Option<&str>) -> AgentChange {
        self.editor()
            .write(&WriteAgentRequest {
                name: name.into(),
                text: text.into(),
                expected_hash: expected.map(str::to_owned),
                rename_from: None,
            })
            .unwrap()
    }

    fn rename(&self, from: &str, to: &str, text: &str, expected: Option<&str>) -> AgentChange {
        self.editor()
            .write(&WriteAgentRequest {
                name: to.into(),
                text: text.into(),
                expected_hash: expected.map(str::to_owned),
                rename_from: Some(from.into()),
            })
            .unwrap()
    }

    fn delete(&self, name: &str, expected: Option<&str>) -> AgentChange {
        self.editor()
            .delete(&DeleteAgentRequest {
                name: name.into(),
                expected_hash: expected.map(str::to_owned),
            })
            .unwrap()
    }

    fn restore(&self, name: &str) -> AgentChange {
        self.editor().restore(name).unwrap()
    }
}

fn hash(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

fn refusal(change: &AgentChange) -> &str {
    change.refusal.as_deref().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

#[test]
fn a_valid_agent_is_created_then_replaced_and_the_next_lookup_reads_each() {
    let world = World::new();
    assert!(
        !world.dir().exists(),
        "the folder is made by the first write"
    );

    let created = world.write("reviewer", REVIEWER, None);
    assert!(created.changed && created.refusal.is_none(), "{created:?}");
    assert_eq!(world.text("reviewer.md").as_deref(), Some(REVIEWER));
    let file = created.file.unwrap();
    assert_eq!(
        (file.name.as_str(), file.hash.as_str()),
        ("reviewer", hash(REVIEWER).as_str())
    );
    assert_eq!(file.fields.unwrap().permission, Some(PermissionMode::Plan));
    let DefinitionLookup::Found(agent) = world.folder.lookup("reviewer") else {
        panic!("the new file is an agent");
    };
    assert_eq!(agent.instructions, "Read the diff.");

    let replaced = world.write("reviewer", REVIEWER_EDITED, Some(&hash(REVIEWER)));
    assert!(replaced.changed, "{replaced:?}");
    assert_eq!(world.text("reviewer.md").as_deref(), Some(REVIEWER_EDITED));
    let DefinitionLookup::Found(agent) = world.folder.lookup("reviewer") else {
        panic!("the replaced file is an agent");
    };
    assert_eq!(agent.instructions, "Read every line.");
    assert!(
        !world.path(".reviewer.md.floti-write").exists(),
        "the file written beside it was renamed over it"
    );

    let again = world.write("reviewer", REVIEWER_EDITED, Some(&hash(REVIEWER_EDITED)));
    assert!(
        !again.changed && again.refusal.is_none(),
        "the same text is no change: {again:?}"
    );
    assert_eq!(
        world.kept("reviewer").len(),
        2,
        "nothing kept for no change"
    );
}

#[test]
fn a_model_or_effort_problem_names_the_backend_whose_entry_it_is() {
    let world = World::new();
    let broken = "---\ndescription: Reads\nmodel:\n  claude-code: opus\n  codex: [a, b]\neffort:\n  codex: loud\n---\nbody\n";

    let refused = world.write("reviewer", broken, None);
    let named: Vec<(Option<&str>, Option<&str>)> = refused
        .problems
        .iter()
        .map(|problem| (problem.field.as_deref(), problem.backend.as_deref()))
        .collect();
    assert_eq!(
        named,
        [
            (Some("model"), Some("codex")),
            (Some("effort"), Some("codex"))
        ],
        "{refused:?}"
    );

    // A problem with the whole map names no backend.
    let flat = "---\ndescription: Reads\nmodel: opus\n---\nbody\n";
    let refused = world.write("reviewer", flat, None);
    assert_eq!(refused.problems.len(), 1, "{refused:?}");
    assert_eq!(refused.problems[0].backend, None);
}

#[test]
fn a_text_the_parser_refuses_is_refused_naming_each_problem_on_its_field_and_line_and_the_file_is_left()
 {
    let world = World::new();
    world.write("reviewer", REVIEWER, None);
    let broken = "---\ndescription: Reads\nworktree: maybe\ndisalowed_tools: Bash\n---\nbody\n";

    let refused = world.write("reviewer", broken, Some(&hash(REVIEWER)));
    assert!(!refused.changed, "{refused:?}");
    assert_eq!(
        refused.problems,
        [
            AgentProblem {
                field: Some("worktree".into()),
                line: Some(3),
                message: "worktree must be yes, no, true or false, not maybe".into(),
                backend: None,
            },
            AgentProblem {
                field: Some("disalowed_tools".into()),
                line: Some(4),
                message: format!(
                    "disalowed_tools is not a field an agent file has (the fields are {})",
                    crate::agent_definitions::parse::FIELDS.join(", ")
                ),
                backend: None,
            },
        ]
    );
    let said = refusal(&refused);
    assert!(
        said.contains("worktree must be") && said.contains("disalowed_tools is not a field"),
        "{said}"
    );
    assert_eq!(world.text("reviewer.md").as_deref(), Some(REVIEWER));
    assert_eq!(
        refused.file.unwrap().hash,
        hash(REVIEWER),
        "the file as it stands"
    );
    assert_eq!(world.kept("reviewer").len(), 1, "only the create was kept");
}

#[test]
fn front_matter_that_does_not_parse_or_is_missing_names_the_line_it_concerns() {
    let world = World::new();
    let unparsed = world.write(
        "reviewer",
        "---\ndescription: ok\nallowed_tools: [Read\nmodel: x\n---\n",
        None,
    );
    let [problem] = unparsed.problems.as_slice() else {
        panic!("one problem: {unparsed:?}");
    };
    assert_eq!(problem.field, None);
    assert!(
        problem.line.is_some_and(|line| (3..=5).contains(&line)),
        "the line is inside the front matter: {problem:?}"
    );
    assert!(
        problem
            .message
            .starts_with("its front matter does not parse")
    );

    let missing = world.write("reviewer", "just a body\n", None);
    assert_eq!(
        missing.problems,
        [AgentProblem {
            field: None,
            line: Some(1),
            message:
                "it has no front matter: an agent file starts with --- lines around its fields"
                    .into(),
            backend: None,
        }]
    );
    assert!(world.text("reviewer.md").is_none(), "nothing was written");
}

#[test]
fn a_stale_hash_or_none_for_a_file_that_is_there_is_a_conflict_and_writes_nothing() {
    let world = World::new();
    world.write("reviewer", REVIEWER, None);
    world.put("reviewer.md", REVIEWER_EDITED);

    let stale = world.write(
        "reviewer",
        "---\ndescription: mine\n---\n",
        Some(&hash(REVIEWER)),
    );
    assert!(!stale.changed, "{stale:?}");
    let conflict = stale.conflict.clone().expect("a conflict");
    assert_eq!(conflict.on_disk_hash, Some(hash(REVIEWER_EDITED)));
    assert_eq!(conflict.on_disk_text.as_deref(), Some(REVIEWER_EDITED));
    assert!(refusal(&stale).contains("changed since it was read"));

    let blind = world.write("reviewer", "---\ndescription: mine\n---\n", None);
    assert!(!blind.changed && blind.conflict.is_some(), "{blind:?}");

    let gone = world.write(
        "scout",
        "---\ndescription: mine\n---\n",
        Some(&hash(REVIEWER)),
    );
    assert_eq!(
        gone.conflict.map(|conflict| conflict.on_disk_hash),
        Some(None),
        "a file that is not there is not the one read"
    );

    assert_eq!(world.text("reviewer.md").as_deref(), Some(REVIEWER_EDITED));
    assert!(world.text("scout.md").is_none());
    assert_eq!(
        world.kept("reviewer").len(),
        1,
        "no conflict keeps anything"
    );
}

#[test]
fn a_name_the_folder_does_not_accept_is_refused_and_writes_nothing() {
    let world = World::new();
    for name in ["Reviewer", "my reviewer", "../reviewer", "reviewer.md", ""] {
        let refused = world.write(name, REVIEWER, None);
        assert!(!refused.changed, "{name}");
        assert!(
            refusal(&refused).contains("lowercase letters, digits and dashes"),
            "{name}: {refused:?}"
        );
    }
    assert!(!world.dir().exists());
}

#[test]
fn the_default_read_from_an_agent_md_from_before_is_written_there() {
    let world = World::new();
    let old = "---\ndescription: The old default\n---\n";
    world.put("agent.md", old);
    let read = world.editor().read("default").unwrap();
    assert_eq!(
        (read.name.as_str(), read.file.as_str()),
        ("default", "agent.md")
    );

    let new = "---\ndescription: The default\n---\n";
    let written = world.write("default", new, Some(&hash(old)));
    assert!(written.changed, "{written:?}");
    assert_eq!(world.text("agent.md").as_deref(), Some(new));
    assert!(world.text("default.md").is_none());
}

/// The names in `dir`, sorted.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_text_is_put_in_place_whole_and_nothing_is_left_beside_it() {
    let world = World::new();
    let path = world.path("scout.md");
    create_file(&path, REVIEWER).unwrap();
    assert_eq!(world.text("scout.md").as_deref(), Some(REVIEWER));
    replace_file(&path, REVIEWER_EDITED).unwrap();
    assert_eq!(world.text("scout.md").as_deref(), Some(REVIEWER_EDITED));

    // A new file is never put over one that is there, and the text written
    // for it does not stay behind.
    let over = create_file(
        &path, "---
---
",
    )
    .unwrap_err();
    assert_eq!(over.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(world.text("scout.md").as_deref(), Some(REVIEWER_EDITED));
    // A write that fails part way leaves the file as it was, and nothing
    // beside it: what a start reads is the old text or the new, whole.
    let half_then_full = |beside: &Path| {
        std::fs::write(
            beside,
            "---
description: half",
        )?;
        Err(io::Error::other("the disk is full"))
    };
    assert!(replace_with(&path, half_then_full).is_err());
    assert_eq!(world.text("scout.md").as_deref(), Some(REVIEWER_EDITED));
    let new = world.path("worker.md");
    assert!(create_with(&new, half_then_full).is_err());
    assert!(!new.exists(), "no half-written file is put in place");
    // A place that cannot take the file leaves nothing either.
    std::fs::create_dir(world.path("folder.md")).unwrap();
    assert!(replace_file(&world.path("folder.md"), REVIEWER).is_err());
    assert_eq!(entries(world.dir()), ["folder.md", "scout.md"]);
}

#[test]
fn a_linked_agent_file_is_written_through_its_link_and_stays_a_link() {
    let world = World::new();
    let dotfiles = world.dirs.folder("dot files");
    let target = dotfiles.join("reviewer.md");
    std::fs::write(&target, REVIEWER).unwrap();
    std::fs::create_dir_all(world.dir()).unwrap();
    link_file(&target, &world.path("reviewer.md"));

    let saved = world.write("reviewer", REVIEWER_EDITED, Some(&hash(REVIEWER)));
    assert!(saved.changed, "{saved:?}");
    assert!(is_link(&world.path("reviewer.md")), "the link stays");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), REVIEWER_EDITED);

    let undone = world.restore("reviewer");
    assert!(undone.changed, "{undone:?}");
    assert!(is_link(&world.path("reviewer.md")), "the link stays");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), REVIEWER);
    assert_eq!(
        entries(&dotfiles),
        ["reviewer.md"],
        "nothing left beside it"
    );
    assert_eq!(entries(world.dir()), ["reviewer.md"]);
}

// ---------------------------------------------------------------------------
// Keeping and restoring
// ---------------------------------------------------------------------------

#[test]
fn a_replace_keeps_the_text_it_replaced_and_a_restore_puts_it_back() {
    let world = World::new();
    world.write("reviewer", REVIEWER, None);
    world.write("reviewer", REVIEWER_EDITED, Some(&hash(REVIEWER)));
    let kept = world.kept("reviewer");
    assert_eq!(
        kept[0].text.as_deref(),
        Some(REVIEWER),
        "the replace kept the old text"
    );
    assert_eq!(kept[0].after_hash, Some(hash(REVIEWER_EDITED)));

    let restored = world.restore("reviewer");
    assert!(restored.changed, "{restored:?}");
    assert_eq!(world.text("reviewer.md").as_deref(), Some(REVIEWER));
    assert_eq!(restored.file.unwrap().hash, hash(REVIEWER));

    // The next restore goes one further back: the create, so the file goes.
    let created = world.restore("reviewer");
    assert!(created.changed && created.file.is_none(), "{created:?}");
    assert!(world.text("reviewer.md").is_none());
    let nothing = world.restore("reviewer");
    assert!(!nothing.changed && refusal(&nothing).contains("kept nothing"));
}

#[test]
fn a_delete_keeps_the_text_and_a_restore_puts_the_file_back() {
    let world = World::new();
    world.put("reviewer.md", REVIEWER);

    let stale = world.delete("reviewer", Some(&hash(REVIEWER_EDITED)));
    assert!(!stale.changed && stale.conflict.is_some(), "{stale:?}");
    // A delete that names no hash names no file it read: an outside edit it
    // never saw is not deleted.
    let unseen = world.delete("reviewer", None);
    assert!(!unseen.changed, "{unseen:?}");
    assert_eq!(
        unseen.conflict.and_then(|conflict| conflict.on_disk_hash),
        Some(hash(REVIEWER))
    );
    assert_eq!(world.text("reviewer.md").as_deref(), Some(REVIEWER));
    assert!(
        world.kept("reviewer").is_empty(),
        "no refused delete keeps anything"
    );

    let deleted = world.delete("reviewer", Some(&hash(REVIEWER)));
    assert!(deleted.changed && deleted.file.is_none(), "{deleted:?}");
    assert!(world.text("reviewer.md").is_none());
    assert_eq!(world.kept("reviewer")[0].text.as_deref(), Some(REVIEWER));

    let restored = world.restore("reviewer");
    assert!(restored.changed, "{restored:?}");
    assert_eq!(world.text("reviewer.md").as_deref(), Some(REVIEWER));
}

#[test]
fn a_rename_keeps_the_old_file_and_a_restore_by_either_name_puts_it_back() {
    for undo_by in ["reviewer", "checker"] {
        let world = World::new();
        world.put("reviewer.md", REVIEWER);

        let renamed = world.rename(
            "reviewer",
            "checker",
            REVIEWER_EDITED,
            Some(&hash(REVIEWER)),
        );
        assert!(renamed.changed, "{renamed:?}");
        assert_eq!(renamed.file.unwrap().name, "checker");
        assert!(world.text("reviewer.md").is_none());
        assert_eq!(world.text("checker.md").as_deref(), Some(REVIEWER_EDITED));

        let restored = world.restore(undo_by);
        assert!(restored.changed, "{undo_by}: {restored:?}");
        assert_eq!(restored.file.unwrap().name, "reviewer");
        assert_eq!(
            world.text("reviewer.md").as_deref(),
            Some(REVIEWER),
            "{undo_by}"
        );
        assert!(world.text("checker.md").is_none(), "{undo_by}");
    }
}

#[test]
fn a_rename_never_writes_over_a_file_and_needs_the_hash_of_the_file_it_renames() {
    let world = World::new();
    world.put("reviewer.md", REVIEWER);
    world.put("checker.md", REVIEWER_EDITED);

    let over = world.rename("reviewer", "checker", REVIEWER, Some(&hash(REVIEWER)));
    assert!(
        !over.changed && refusal(&over).contains("never writes over"),
        "{over:?}"
    );
    let stale = world.rename("reviewer", "scout", REVIEWER, Some(&hash(REVIEWER_EDITED)));
    assert!(!stale.changed && stale.conflict.is_some(), "{stale:?}");

    assert_eq!(world.text("reviewer.md").as_deref(), Some(REVIEWER));
    assert_eq!(world.text("checker.md").as_deref(), Some(REVIEWER_EDITED));
    assert!(world.text("scout.md").is_none());
    assert!(world.kept("reviewer").is_empty());
}

#[test]
fn a_restore_puts_nothing_back_over_an_edit_made_since_the_change() {
    let world = World::new();
    world.put("reviewer.md", REVIEWER);
    world.write("reviewer", REVIEWER_EDITED, Some(&hash(REVIEWER)));
    let outside = "---\ndescription: Edited by hand\n---\n";
    world.put("reviewer.md", outside);

    let refused = world.restore("reviewer");
    assert!(!refused.changed, "{refused:?}");
    assert!(
        refusal(&refused).contains("changed since it was saved"),
        "{refused:?}"
    );
    assert_eq!(
        refused.conflict.unwrap().on_disk_text.as_deref(),
        Some(outside)
    );
    assert_eq!(world.text("reviewer.md").as_deref(), Some(outside));
    assert_eq!(world.kept("reviewer").len(), 1, "the change is still kept");
}

#[test]
fn only_the_newest_changes_of_an_agent_are_kept() {
    let world = World::new();
    let texts: Vec<String> = (0..KEPT_PER_AGENT + 5)
        .map(|n| format!("---\ndescription: version {n}\n---\n"))
        .collect();
    world.write("reviewer", &texts[0], None);
    for pair in texts.windows(2) {
        let written = world.write("reviewer", &pair[1], Some(&hash(&pair[0])));
        assert!(written.changed, "{written:?}");
    }
    let kept = world.kept("reviewer");
    assert_eq!(kept.len(), KEPT_PER_AGENT as usize);
    assert_eq!(
        kept[0].text.as_deref(),
        Some(texts[texts.len() - 2].as_str()),
        "the newest kept is the text before the last write"
    );
}

// ---------------------------------------------------------------------------
// Shipped agents
// ---------------------------------------------------------------------------

#[test]
fn a_missing_shipped_agent_is_added_and_one_whose_file_is_there_is_never_written_over() {
    let world = World::new();
    let (file, text) = shipped("worker").expect("worker is shipped");

    let added = world.editor().add_shipped("worker").unwrap();
    assert!(added.changed, "{added:?}");
    assert_eq!(world.text(file).as_deref(), Some(text));

    world.put("reviewer.md", REVIEWER);
    let refused = world.editor().add_shipped("reviewer").unwrap();
    assert!(!refused.changed && refusal(&refused).contains("never writes over"));
    assert_eq!(world.text("reviewer.md").as_deref(), Some(REVIEWER));

    let unknown = world.editor().add_shipped("scout").unwrap();
    assert!(!unknown.changed);
    assert!(
        refusal(&unknown).contains("lead, researcher, reviewer and worker"),
        "{unknown:?}"
    );

    // Undone, the added file goes again.
    let undone = world.restore("worker");
    assert!(undone.changed, "{undone:?}");
    assert!(world.text(file).is_none());
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

#[test]
fn a_bad_file_is_read_with_each_value_as_written_on_its_line_and_its_problems() {
    let world = World::new();
    let text = "---\n\
                description: Reads\n\
                effort:\n  claude-code: hard\n\
                allowed_tools:\n  - Read\n  - Bash(git log:*)\n\
                # a comment\n\
                class: light\n\
                ---\n\
                The body.\n";
    world.put("reviewer.md", text);

    let read = world.editor().read("reviewer").unwrap();
    assert_eq!(read.text, text);
    assert_eq!(read.hash, hash(text));
    assert!(read.fields.is_none(), "a bad file has no parsed fields");
    let value = |field: &str, line: u32, value: &str| AgentWrittenValue {
        field: field.into(),
        line,
        value: value.into(),
    };
    assert_eq!(
        read.values,
        [
            value("description", 2, "Reads"),
            value("effort", 3, "claude-code: hard"),
            value("allowed_tools", 5, "- Read\n- Bash(git log:*)"),
            value("class", 9, "light"),
        ]
    );
    assert_eq!(
        read.problems,
        [AgentProblem {
            field: Some("effort".into()),
            line: Some(3),
            message: "effort for claude-code must be low, medium, high, xhigh or max".into(),
            backend: Some("claude-code".into()),
        }]
    );
    assert_eq!(read.body, "The body.\n");
}

#[test]
fn a_good_file_is_read_with_its_fields_and_each_retired_line_on_its_line() {
    let world = World::new();
    world.put(
        "worker.md",
        "---\ndescription: Builds\nclass: heavy\nbackend: claude-code\n---\nBuild it.\n",
    );
    let read = world.editor().read("worker").unwrap();
    assert!(read.problems.is_empty(), "{read:?}");
    assert_eq!(read.fields.unwrap().backends, ["claude-code"]);
    assert_eq!(
        read.unused_lines,
        [floti_protocol::channels::agents::AgentUnusedLine {
            field: "class".into(),
            line: 3,
        }]
    );
    let missing = world.editor().read("scout").unwrap_err();
    assert!(missing.contains("no agent file scout.md"), "{missing}");
}

// ---------------------------------------------------------------------------
// A host that writes no config
// ---------------------------------------------------------------------------

#[test]
fn a_read_only_host_refuses_every_change_with_its_sentence_and_still_reads() {
    let world = World::new();
    world.put("reviewer.md", REVIEWER);
    let editor = world.read_only();

    let changes = [
        editor
            .write(&WriteAgentRequest {
                name: "reviewer".into(),
                text: REVIEWER_EDITED.into(),
                expected_hash: Some(hash(REVIEWER)),
                rename_from: None,
            })
            .unwrap(),
        editor
            .delete(&DeleteAgentRequest {
                name: "reviewer".into(),
                expected_hash: None,
            })
            .unwrap(),
        editor.restore("reviewer").unwrap(),
        editor.add_shipped("worker").unwrap(),
    ];
    for change in &changes {
        assert!(!change.changed, "{change:?}");
        assert_eq!(change.refusal.as_deref(), Some(READ_ONLY));
    }
    assert!(READ_ONLY.contains("does not change its config folder"));
    assert_eq!(world.text("reviewer.md").as_deref(), Some(REVIEWER));
    assert!(world.text("worker.md").is_none());
    assert_eq!(editor.read("reviewer").unwrap().hash, hash(REVIEWER));
}

#[test]
fn a_host_serving_the_shipped_agents_with_no_folder_reads_them() {
    let world = World::new();
    let folder = FolderDefinitions::with_shipped(world.dir());
    let editor = AgentEditor::new(&folder, world.store.connection(), true);
    let read = editor.read("lead").unwrap();
    assert!(!read.in_folder);
    assert_eq!(read.text, shipped("lead").unwrap().1);
    assert!(!world.dir().exists(), "nothing was written");
}
