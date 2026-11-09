//! Agent files read, listed and seeded: what a file may say, what makes it
//! bad, that every call reads the folder afresh, what the first start writes,
//! and how a folder still called `roles` becomes `agents`.

use std::path::Path;

use floti_protocol::BackendId;
use floti_protocol::channels::common::PermissionMode;
use floti_protocol::channels::sessions::EffortLevel;

use super::folder::{
    FolderDefinitions, agents_folder, prepare_agents_folder, prepare_agents_folder_with,
    read_agents_folder,
};
use super::parse::parse_definition;
use super::seed::{SHIPPED_AGENTS, seed_agents};
use super::*;
use crate::agent::PlainCommandBackend;
use crate::testkit::TestDirs;

fn good(name: &str, source: &str) -> AgentDefinition {
    parse_definition(name, source).unwrap_or_else(|problem| panic!("{name} is bad: {problem}"))
}

fn bad(name: &str, source: &str) -> String {
    parse_definition(name, source).expect_err("the file is bad")
}

fn write(dir: &Path, file: &str, content: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(file), content).unwrap();
}

fn ids(agent: &AgentDefinition) -> Vec<&str> {
    agent.backends.iter().map(BackendId::as_str).collect()
}

// ---------------------------------------------------------------------------
// A file read
// ---------------------------------------------------------------------------

#[test]
fn a_file_with_every_field_reads_as_that_agent_and_its_body_is_the_instructions() {
    let agent = good(
        "reviewer",
        "---\n\
         description: Read-only review.\n\
         profile: Acme\n\
         backend: claude-code, other-agent\n\
         model:\n  claude-code: opus\n  other-agent: small\n\
         effort:\n  claude-code: high\n\
         permission: plan\n\
         worktree: yes\n\
         can_start: true\n\
         allowed_tools: Read, Bash(git log --format=a,b:*)\n\
         disallowed_tools:\n  - Edit\n  - Write\n\
         command: acme\n\
         submit: enter\n\
         stop: ctrl-c\n\
         ---\n\
         \n\
         Read the code first.\n\n\
         Then report.\n",
    );
    assert_eq!(agent.name, "reviewer");
    assert_eq!(agent.description, "Read-only review.");
    assert_eq!(agent.profile.as_deref(), Some("Acme"));
    assert_eq!(ids(&agent), ["claude-code", "other-agent"]);
    assert_eq!(agent.model_for(&BackendId::claude_code()), Some("opus"));
    assert_eq!(
        agent.model_for(&BackendId::new("other-agent")),
        Some("small")
    );
    assert_eq!(
        agent.effort_for(&BackendId::claude_code()),
        Some(EffortLevel::High)
    );
    assert_eq!(agent.effort_for(&BackendId::new("other-agent")), None);
    assert_eq!(agent.permission, Some(PermissionMode::Plan));
    assert!(agent.worktree && agent.can_start);
    // A comma inside parentheses belongs to the rule.
    assert_eq!(
        agent.allowed_tools,
        ["Read", "Bash(git log --format=a,b:*)"]
    );
    assert_eq!(agent.disallowed_tools, ["Edit", "Write"]);
    // Kept for a plain command line: its submit and stop words are keys.
    assert_eq!(agent.command.as_deref(), Some("acme"));
    assert_eq!(agent.submit.as_deref(), Some("enter"));
    assert_eq!(agent.stop.as_deref(), Some("ctrl-c"));
    assert_eq!(agent.instructions, "Read the code first.\n\nThen report.");
}

#[test]
fn a_class_line_is_accepted_with_any_value_ignored_and_kept_as_a_line_no_longer_used() {
    for value in ["light", "heavy", "medium", "", "[1, 2]"] {
        let agent = good(
            "builder",
            &format!("---\nbackend: claude-code\nclass: {value}\nworktree: yes\n---\nBuild.\n"),
        );
        assert_eq!(agent.unused_lines, ["class"], "class: {value}");
        // Everything else in the file still reads.
        assert_eq!(ids(&agent), ["claude-code"], "class: {value}");
        assert!(agent.worktree, "class: {value}");
        assert_eq!(agent.instructions, "Build.", "class: {value}");
    }
    // An agent with a class line is the agent without it, but for that line.
    let with = good("a", "---\nclass: heavy\npermission: plan\n---\nGo.\n");
    let without = good("a", "---\npermission: plan\n---\nGo.\n");
    assert_eq!(
        AgentDefinition {
            unused_lines: Vec::new(),
            ..with
        },
        without
    );
    // It is not offered as one of the fields an agent has.
    let typo = bad("a", "---\nclas: heavy\n---\n");
    assert!(!typo.contains("class"), "{typo}");
}

#[test]
fn a_file_with_no_fields_is_an_agent_with_the_defaults() {
    let agent = good("plain", "---\n---\nJust do it.\n");
    assert!(agent.backends.is_empty(), "every installed agent backend");
    assert!(!agent.worktree && !agent.can_start);
    assert!(agent.unused_lines.is_empty());
    assert_eq!(agent.permission, None);
    assert_eq!(agent.instructions, "Just do it.");

    let empty = good("empty", "---\ndescription: Nothing else.\n---\n");
    assert_eq!(empty.instructions, "");
}

#[test]
fn backends_are_one_a_comma_list_or_a_yaml_list_and_an_id_that_is_not_installed_is_valid() {
    assert_eq!(
        ids(&good("a", "---\nbackend: claude-code\n---\n")),
        ["claude-code"]
    );
    assert_eq!(
        ids(&good("b", "---\nbackend: codex, claude-code\n---\n")),
        ["codex", "claude-code"]
    );
    assert_eq!(
        ids(&good(
            "c",
            "---\nbackend:\n  - Other-Agent\n  - codex\n---\n"
        )),
        ["other-agent", "codex"]
    );
}

#[test]
fn permission_is_written_with_hyphens_spaces_or_camel_case() {
    for (written, mode) in [
        ("plan", PermissionMode::Plan),
        ("dont-ask", PermissionMode::DontAsk),
        ("dontAsk", PermissionMode::DontAsk),
        ("manual", PermissionMode::Manual),
        ("accept-edits", PermissionMode::AcceptEdits),
        ("accept edits", PermissionMode::AcceptEdits),
        ("acceptEdits", PermissionMode::AcceptEdits),
        ("auto", PermissionMode::Auto),
        ("bypass-permissions", PermissionMode::BypassPermissions),
        ("bypassPermissions", PermissionMode::BypassPermissions),
    ] {
        let agent = good("p", &format!("---\npermission: {written}\n---\n"));
        assert_eq!(agent.permission, Some(mode), "{written}");
    }
}

#[test]
fn flags_are_yes_no_true_or_false() {
    for (written, expected) in [
        ("yes", true),
        ("no", false),
        ("true", true),
        ("false", false),
    ] {
        let agent = good(
            "f",
            &format!("---\nworktree: {written}\ncan_start: {written}\n---\n"),
        );
        assert_eq!(
            (agent.worktree, agent.can_start),
            (expected, expected),
            "{written}"
        );
    }
}

#[test]
fn a_file_that_is_not_exactly_an_agent_is_bad_and_says_what_is_wrong() {
    // No front matter at all.
    assert!(bad("a", "Just some text.\n").contains("no front matter"));
    // A front matter that never ends.
    assert!(bad("a", "---\ndescription: x\n").contains("front matter"));
    // A front matter that does not parse.
    assert!(bad("a", "---\ndescription: [unclosed\n---\n").contains("does not parse"));
    // A key no agent has: a typo must never quietly drop a rule.
    let typo = bad("a", "---\ndisalowed_tools: Edit\n---\n");
    assert!(typo.contains("disalowed_tools"), "{typo}");
    assert!(
        typo.contains("disallowed_tools"),
        "the fields are listed: {typo}"
    );
    // A value outside its field's choices.
    let effort = bad("a", "---\neffort:\n  claude-code: loud\n---\n");
    assert!(effort.contains("effort for claude-code"), "{effort}");
    let permission = bad("a", "---\npermission: always\n---\n");
    assert!(permission.contains("permission must be"), "{permission}");
    assert!(bad("a", "---\nworktree: maybe\n---\n").contains("worktree must be"));
    assert!(bad("a", "---\nmodel: opus\n---\n").contains("model is a map"));
    assert!(bad("a", "---\nbackend: \"\"\n---\n").contains("backend"));
    // A submit or stop word that is not a key would press nothing.
    let submit = bad("a", "---\nsubmit: hit it\n---\n");
    assert!(submit.contains("submit: hit it is not a key"), "{submit}");
    let stop = bad("a", "---\nstop: ctrl-9\n---\n");
    assert!(stop.contains("stop: ctrl-9 is not a key"), "{stop}");
    // Every problem is named, not only the first.
    let two = bad("a", "---\nworktree: x\npermission: y\n---\n");
    assert!(
        two.contains("worktree") && two.contains("permission"),
        "{two}"
    );
}

/// Each problem with `source`, as `(field, line)`.
fn placed(source: &str) -> Vec<(Option<String>, Option<u32>)> {
    super::parse::parse_agent("a", source)
        .expect_err("the file is bad")
        .into_iter()
        .map(|problem| (problem.field, problem.line))
        .collect()
}

#[test]
fn each_problem_names_the_field_and_the_line_of_the_file_it_concerns() {
    let field = |name: &str, line: u32| (Some(name.to_owned()), Some(line));
    assert_eq!(
        placed(
            "---
description: x
worktree: maybe

permission: always
---
"
        ),
        [field("worktree", 3), field("permission", 5)]
    );
    // A map's line is the line of its field, whichever entry is wrong.
    assert_eq!(
        placed(
            "---
effort:
  claude-code: high
  other-agent: loud
---
"
        ),
        [field("effort", 2)]
    );
    // A key that is not text is placed by how the file writes it.
    assert_eq!(
        placed(
            "---
1: one
---
"
        ),
        [field("1", 2)]
    );
    // A quoted key is the field it names.
    assert_eq!(
        placed(
            "---
\"worktree\": maybe
---
"
        ),
        [field("worktree", 2)]
    );
    // A problem with the whole front matter names its line and no field.
    assert_eq!(
        placed(
            "Just some text.
"
        ),
        [(None, Some(1))]
    );
    assert_eq!(
        placed(
            "

---
description: x
"
        ),
        [(None, Some(3))]
    );
    let unparsed = placed(
        "---
description: ok
model: [unclosed
---
",
    );
    let [(None, Some(line))] = unparsed.as_slice() else {
        panic!("one problem with the front matter: {unparsed:?}");
    };
    assert!((2..=4).contains(line), "inside the front matter: {line}");
}

#[test]
fn each_value_is_read_as_the_file_writes_it_whether_or_not_it_parses() {
    let reading = super::parse::read_agent_file(
        "a",
        "---
description:   Reads a change  
model:
  claude-code: opus

  other-agent: big
\"permission\": plan
---
body
",
    );
    let values: Vec<(&str, u32, &str)> = reading
        .values
        .iter()
        .map(|value| (value.field.as_str(), value.line, value.value.as_str()))
        .collect();
    assert_eq!(
        values,
        [
            ("description", 2, "Reads a change"),
            (
                "model",
                3,
                "claude-code: opus

other-agent: big"
            ),
            ("permission", 7, "plan"),
        ]
    );
    assert!(reading.agent.is_ok(), "{:?}", reading.agent);

    let broken = super::parse::read_agent_file(
        "a",
        "---
worktree: [maybe
model: x
---
",
    );
    assert!(broken.agent.is_err());
    assert_eq!(
        broken
            .values
            .iter()
            .map(|value| value.field.as_str())
            .collect::<Vec<_>>(),
        ["worktree", "model"],
        "a front matter that does not parse is still read as written"
    );
}

// ---------------------------------------------------------------------------
// The folder, read afresh
// ---------------------------------------------------------------------------

#[test]
fn the_agents_folder_is_agents_in_the_config_folder() {
    let dirs = TestDirs::new();
    assert_eq!(agents_folder(&dirs.folders()), dirs.config.join("agents"));
}

#[test]
fn a_listing_has_the_good_agents_by_name_then_each_bad_file_with_its_problem() {
    let dirs = TestDirs::new();
    let dir = dirs.config.join("agents");
    write(&dir, "zeta.md", "---\ndescription: Last.\n---\nz\n");
    write(&dir, "alpha.md", "---\ndescription: First.\n---\na\n");
    write(
        &dir,
        "broken.md",
        "---\neffort:\n  claude-code: loud\n---\n",
    );
    write(&dir, "no-front.md", "nothing\n");
    // Not agent files: other extensions, other names and folders.
    write(&dir, "notes.txt", "---\n---\n");
    write(&dir, "Upper Case.md", "---\n---\n");
    write(&dir, "Shouting.md", "---\n---\n");
    std::fs::create_dir_all(dir.join("folder.md")).unwrap();

    let listing = FolderDefinitions::new(&dir).listing();
    let names: Vec<&str> = listing
        .agents
        .iter()
        .map(|agent| agent.name.as_str())
        .collect();
    assert_eq!(names, ["alpha", "zeta"]);
    let files: Vec<&str> = listing.bad.iter().map(|bad| bad.file.as_str()).collect();
    assert_eq!(files, ["broken.md", "no-front.md"]);
    assert!(listing.bad[0].problem.contains("effort for claude-code"));
    assert!(listing.bad[1].problem.contains("no front matter"));
}

#[test]
fn no_folder_is_no_agents() {
    let dirs = TestDirs::new();
    let agents = FolderDefinitions::new(dirs.config.join("agents"));
    assert_eq!(agents.listing(), DefinitionListing::default());
    assert_eq!(agents.lookup("reviewer"), DefinitionLookup::Missing);
}

#[test]
fn every_call_reads_the_folder_afresh_so_an_edit_an_add_and_a_delete_show_at_once() {
    let dirs = TestDirs::new();
    let dir = dirs.config.join("agents");
    let agents = FolderDefinitions::new(&dir);

    write(&dir, "scout.md", "---\ndescription: One.\n---\nfirst\n");
    let DefinitionLookup::Found(first) = agents.lookup("scout") else {
        panic!("scout is an agent");
    };
    assert_eq!(
        (first.description.as_str(), first.instructions.as_str()),
        ("One.", "first")
    );

    // Edited: the next call has the new text.
    write(
        &dir,
        "scout.md",
        "---\ndescription: Two.\nworktree: yes\n---\nsecond\n",
    );
    let DefinitionLookup::Found(second) = agents.lookup("scout") else {
        panic!("scout is still an agent");
    };
    assert_eq!(second.description, "Two.");
    assert_eq!(second.instructions, "second");
    assert!(second.worktree);

    // Broken: the lookup says why, naming the file.
    write(&dir, "scout.md", "---\nworktree: maybe\n---\n");
    let DefinitionLookup::Bad(broken) = agents.lookup("scout") else {
        panic!("scout is bad now");
    };
    assert_eq!(broken.file, "scout.md");
    assert!(broken.problem.contains("worktree must be"));
    assert!(agents.listing().agents.is_empty());

    // Added and deleted.
    write(&dir, "new-one.md", "---\n---\nbody\n");
    assert_eq!(agents.listing().agents.len(), 1);
    std::fs::remove_file(dir.join("scout.md")).unwrap();
    std::fs::remove_file(dir.join("new-one.md")).unwrap();
    assert_eq!(agents.lookup("scout"), DefinitionLookup::Missing);
    assert_eq!(agents.listing(), DefinitionListing::default());
}

#[test]
fn a_lookup_ignores_the_case_of_the_name_and_a_name_that_cannot_be_an_agent_is_missing() {
    let dirs = TestDirs::new();
    let dir = dirs.config.join("agents");
    write(&dir, "reviewer.md", "---\n---\nx\n");
    let agents = FolderDefinitions::new(&dir);
    assert!(matches!(
        agents.lookup("Reviewer"),
        DefinitionLookup::Found(_)
    ));
    assert_eq!(agents.lookup("../reviewer"), DefinitionLookup::Missing);
    assert_eq!(agents.lookup("re viewer"), DefinitionLookup::Missing);
    assert_eq!(agents.lookup(""), DefinitionLookup::Missing);
}

#[test]
fn a_default_md_is_the_default_agent() {
    let dirs = TestDirs::new();
    let dir = dirs.config.join("agents");
    write(
        &dir,
        "default.md",
        "---\npermission: plan\n---\nBe careful.\n",
    );
    let DefinitionLookup::Found(agent) = FolderDefinitions::new(&dir).lookup(DEFAULT_AGENT_NAME)
    else {
        panic!("default.md is an agent");
    };
    assert_eq!(DEFAULT_AGENT_NAME, "default");
    assert_eq!(agent.name, "default");
    assert_eq!(agent.permission, Some(PermissionMode::Plan));
}

// ---------------------------------------------------------------------------
// What an agent asks of the host
// ---------------------------------------------------------------------------

fn plain_command() -> std::sync::Arc<dyn AgentBackend> {
    std::sync::Arc::new(PlainCommandBackend::new(
        BackendId::new("my-cli"),
        "My CLI",
        "my-cli",
        Vec::<&str>::new(),
    ))
}

#[test]
fn a_plain_command_line_runs_no_agent_and_an_agent_that_lists_no_backend_never_picks_one() {
    let host = vec![plain_command()];
    assert!(
        AgentDefinition::new("any", [])
            .backends_on(&host)
            .is_empty()
    );
    assert!(default_definition(&host).backends.is_empty());
    // Even when it is named, a plain command line takes no task.
    let named = AgentDefinition::new("named", [BackendId::new("my-cli")]);
    assert_eq!(named.backends_on(&host).len(), 1);
    let refusal = named.refusal_on(host[0].as_ref(), None).expect("a refusal");
    assert!(
        refusal.contains("my-cli") && refusal.contains("plain command line"),
        "{refusal}"
    );
}

#[test]
fn an_agent_that_names_a_backend_the_host_does_not_have_runs_on_none() {
    let host = vec![plain_command()];
    let codex = AgentDefinition::new("codex-only", [BackendId::new("codex")]);
    assert!(codex.backends_on(&host).is_empty());
}

// ---------------------------------------------------------------------------
// A folder still called roles
// ---------------------------------------------------------------------------

/// Every file under `dir`, by its path from `dir`, with its bytes.
fn files_in(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push((name, std::fs::read(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn a_roles_folder_with_no_agents_folder_is_moved_whole_to_agents_and_not_seeded() {
    let dirs = TestDirs::new();
    let roles = dirs.config.join("roles");
    write(
        &roles,
        "reviewer.md",
        "---\ndescription: Mine.\n---\nMy words.\r\n",
    );
    write(&roles, "notes.txt", "not an agent");
    write(&roles.join("drafts"), "idea.md", "---\n---\n");
    let before = files_in(&roles);

    let prepared = prepare_agents_folder(&dirs.config);
    let agents = dirs.config.join("agents");
    assert_eq!(prepared.dir, agents);
    assert!(!prepared.seed);
    assert!(!roles.exists(), "the roles folder is gone");
    assert_eq!(files_in(&agents), before, "every file as it was");
    let said = prepared.log.expect("the move is logged");
    assert!(said.contains("moved") && said.contains("roles"), "{said}");

    // The next start finds only agents and says nothing.
    let again = prepare_agents_folder(&dirs.config);
    assert_eq!((again.dir, again.seed, again.log), (agents, false, None));
}

#[test]
fn an_agents_folder_beside_a_roles_folder_is_read_and_the_roles_folder_left_alone() {
    let dirs = TestDirs::new();
    let roles = dirs.config.join("roles");
    let agents = dirs.config.join("agents");
    write(&roles, "old.md", "---\n---\nold\n");
    write(&agents, "new.md", "---\n---\nnew\n");
    let roles_before = files_in(&roles);
    let agents_before = files_in(&agents);

    let prepared = prepare_agents_folder(&dirs.config);
    assert_eq!(prepared.dir, agents);
    assert!(!prepared.seed);
    assert_eq!(files_in(&roles), roles_before);
    assert_eq!(files_in(&agents), agents_before);
    let said = prepared.log.expect("both folders are named once");
    assert!(
        said.contains(&roles.display().to_string()) && said.contains("left as it is"),
        "{said}"
    );
}

#[test]
fn with_neither_folder_the_agents_folder_is_to_be_seeded() {
    let dirs = TestDirs::new();
    let prepared = prepare_agents_folder(&dirs.config);
    assert_eq!(prepared.dir, dirs.config.join("agents"));
    assert!(prepared.seed);
    assert_eq!(prepared.log, None);
}

#[test]
fn a_roles_folder_that_cannot_be_moved_is_read_where_it_is_and_never_seeded_over() {
    let dirs = TestDirs::new();
    let roles = dirs.config.join("roles");
    write(&roles, "reviewer.md", "---\n---\nmine\n");
    let before = files_in(&roles);

    let prepared = prepare_agents_folder_with(&dirs.config, |_, _| {
        Err(std::io::Error::other("the folder is in use"))
    });
    assert_eq!(prepared.dir, roles);
    assert!(
        !prepared.seed,
        "a folder that is there is never seeded over"
    );
    assert_eq!(files_in(&roles), before);
    assert!(!dirs.config.join("agents").exists());
    let said = prepared.log.expect("the failure is logged");
    assert!(said.contains("the folder is in use"), "{said}");
}

// ---------------------------------------------------------------------------
// What the first start writes
// ---------------------------------------------------------------------------

#[test]
fn an_absent_folder_gets_the_three_shipped_agents_and_each_reads_as_a_good_agent() {
    let dirs = TestDirs::new();
    let dir = dirs.config.join("agents");
    let seeded = seed_agents(&dir);
    assert!(seeded.seeded && seeded.problem.is_none(), "{seeded:?}");
    assert_eq!(seeded.created, ["lead.md", "reviewer.md", "worker.md"]);

    let listing = FolderDefinitions::new(&dir).listing();
    assert!(listing.bad.is_empty(), "{:?}", listing.bad);
    let names: Vec<&str> = listing
        .agents
        .iter()
        .map(|agent| agent.name.as_str())
        .collect();
    assert_eq!(names, ["lead", "reviewer", "worker"]);
}

#[test]
fn a_folder_that_is_there_is_never_written_to_whatever_it_holds() {
    let dirs = TestDirs::new();

    // Empty stays empty.
    let empty = dirs.config.join("empty agents");
    std::fs::create_dir_all(&empty).unwrap();
    let seeded = seed_agents(&empty);
    assert!(!seeded.seeded && seeded.created.is_empty());
    assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);

    // An edited file is not rewritten, and one the user deleted stays deleted.
    let dir = dirs.config.join("agents");
    seed_agents(&dir);
    let edited = "---\ndescription: Mine.\n---\nMy own words.\n";
    std::fs::write(dir.join("worker.md"), edited).unwrap();
    std::fs::remove_file(dir.join("lead.md")).unwrap();
    let again = seed_agents(&dir);
    assert!(!again.seeded && again.created.is_empty());
    assert_eq!(
        std::fs::read_to_string(dir.join("worker.md")).unwrap(),
        edited
    );
    assert!(!dir.join("lead.md").exists());
    assert!(dir.join("reviewer.md").exists());

    // Deleting the folder brings the originals back at the next start.
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(seed_agents(&dir).seeded);
    assert!(dir.join("lead.md").exists());
}

#[test]
fn a_folder_that_cannot_be_made_is_a_problem_to_log_and_not_a_panic() {
    let dirs = TestDirs::new();
    // A file where the config folder's parent should be.
    let blocker = dirs.config.join("blocker");
    std::fs::write(&blocker, "a file").unwrap();
    let seeded = seed_agents(&blocker.join("agents"));
    assert!(!seeded.seeded);
    assert!(
        seeded
            .problem
            .as_deref()
            .is_some_and(|problem| problem.contains("agents folder")),
        "{seeded:?}"
    );
}

// ---------------------------------------------------------------------------
// What the shipped agents are
// ---------------------------------------------------------------------------

fn shipped(name: &str) -> AgentDefinition {
    let (_, source) = SHIPPED_AGENTS
        .iter()
        .find(|(file, _)| *file == format!("{name}.md"))
        .expect("a shipped agent");
    good(name, source)
}

#[test]
fn the_reviewer_reads_and_reports_without_editing() {
    let agent = shipped("reviewer");
    assert_eq!(agent.permission, Some(PermissionMode::Auto));
    assert!(!agent.can_start && !agent.worktree);
    assert_eq!(agent.disallowed_tools, ["Edit", "Write", "NotebookEdit"]);
    assert_eq!(agent.model_for(&BackendId::claude_code()), Some("opus"));
    assert_eq!(
        agent.effort_for(&BackendId::claude_code()),
        Some(EffortLevel::High)
    );
    assert!(!agent.instructions.is_empty());
}

#[test]
fn the_worker_edits_in_a_worktree_of_its_own_and_cannot_start_agents_or_create_items() {
    let agent = shipped("worker");
    assert_eq!(agent.permission, Some(PermissionMode::Auto));
    assert!(agent.worktree && !agent.can_start);
    assert_eq!(agent.disallowed_tools, ["mcp__helm-plugin-trackr__create"]);
    for rule in [
        "Touch only your own item",
        "Also fixed:",
        "Found:",
        "Blocked",
    ] {
        assert!(agent.instructions.contains(rule), "{rule}");
    }
}

#[test]
fn the_lead_starts_agents_does_not_edit_and_keeps_trackr_in_a_voice() {
    let agent = shipped("lead");
    assert!(agent.can_start && !agent.worktree);
    assert_eq!(agent.permission, Some(PermissionMode::Auto));
    assert_eq!(agent.disallowed_tools, ["Edit", "Write", "NotebookEdit"]);
    for section in ["## Keeping Trackr", "## Voice"] {
        assert!(agent.instructions.contains(section), "{section}");
    }
    assert!(agent.instructions.contains("robotic and then right"));
}

#[test]
fn the_shipped_agents_name_no_product_no_path_no_handle_and_only_made_up_item_ids() {
    for (file, source) in SHIPPED_AGENTS {
        // Nothing that ties them to one machine, person or product: no
        // product of this repository, no agent but the one they run on, no
        // path, address or handle.
        for word in [
            "Helm", "Floti", "codex", "C:\\", "/Users/", "/home/", "http:", "https:", "@",
        ] {
            assert!(!source.contains(word), "{file} names {word}");
        }
        // An item id looks like ABC-12, and every one here is made up.
        for token in source.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
            let looks_like_an_id = token.split_once('-').is_some_and(|(letters, digits)| {
                letters.len() >= 2
                    && letters.chars().all(|c| c.is_ascii_uppercase())
                    && !digits.is_empty()
                    && digits.chars().all(|c| c.is_ascii_digit())
            });
            assert!(
                !looks_like_an_id || token.starts_with("ABC-"),
                "{file}: {token}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The default under its earlier name
// ---------------------------------------------------------------------------

#[test]
fn either_of_the_defaults_names_is_the_default() {
    for name in ["default", "Default", "agent", " AGENT "] {
        assert!(is_default_name(name), "{name}");
    }
    for name in ["agents", "reviewer", ""] {
        assert!(!is_default_name(name), "{name}");
    }
}

#[test]
fn an_agent_md_with_no_default_md_is_the_default_under_its_name_and_listed_once() {
    let dirs = TestDirs::new();
    let dir = dirs.config.join("agents");
    write(
        &dir,
        "agent.md",
        "---\ndescription: Old.\npermission: plan\n---\nOld words.\n",
    );
    write(&dir, "reviewer.md", "---\n---\nReview.\n");
    let agents = FolderDefinitions::new(&dir);

    let listing = agents.listing();
    let names: Vec<&str> = listing
        .agents
        .iter()
        .map(|agent| agent.name.as_str())
        .collect();
    assert_eq!(names, ["default", "reviewer"]);
    for asked in ["default", "agent"] {
        let DefinitionLookup::Found(found) = agents.lookup(asked) else {
            panic!("{asked} finds agent.md");
        };
        assert_eq!(found.name, "default", "{asked}");
        assert_eq!(found.instructions, "Old words.", "{asked}");
        assert_eq!(found.permission, Some(PermissionMode::Plan), "{asked}");
    }
    // The trait's own lookup, over a fixed list, finds the default by
    // either name too.
    let fixed = FixedDefinitions(vec![AgentDefinition::new("default", [])]);
    assert!(matches!(fixed.lookup("agent"), DefinitionLookup::Found(_)));
}

#[test]
fn beside_a_default_md_an_agent_md_is_not_read() {
    let dirs = TestDirs::new();
    let dir = dirs.config.join("agents");
    write(&dir, "agent.md", "---\n---\nOld words.\n");
    write(&dir, "default.md", "---\n---\nNew words.\n");
    let agents = FolderDefinitions::new(&dir);
    let listing = agents.listing();
    let names: Vec<&str> = listing
        .agents
        .iter()
        .map(|agent| agent.name.as_str())
        .collect();
    assert_eq!(names, ["default"]);
    assert_eq!(listing.agents[0].instructions, "New words.");
    let DefinitionLookup::Found(found) = agents.lookup("agent") else {
        panic!("agent finds the default");
    };
    assert_eq!(found.instructions, "New words.");
}

#[test]
fn a_broken_agent_md_with_no_default_md_is_the_broken_default_naming_its_file() {
    let dirs = TestDirs::new();
    let dir = dirs.config.join("agents");
    write(&dir, "agent.md", "---\nworktree: maybe\n---\n");
    let agents = FolderDefinitions::new(&dir);
    let DefinitionLookup::Bad(bad) = agents.lookup("default") else {
        panic!("a bad default");
    };
    assert_eq!(bad.file, "agent.md");
    assert_eq!(agents.listing().bad, [bad]);
}

// ---------------------------------------------------------------------------
// A host that may not write its config folder
// ---------------------------------------------------------------------------

#[test]
fn a_read_only_host_reads_agents_else_roles_where_it_is_and_never_seeds() {
    let dirs = TestDirs::new();
    let roles = dirs.config.join("roles");
    write(&roles, "reviewer.md", "---\n---\nmine\n");
    let before = files_in(&roles);

    let prepared = read_agents_folder(&dirs.config);
    assert_eq!(prepared.dir, roles);
    assert!(!prepared.seed && !prepared.shipped);
    assert_eq!(files_in(&roles), before, "nothing moved or written");
    assert!(!dirs.config.join("agents").exists());

    let agents = dirs.config.join("agents");
    write(&agents, "lead.md", "---\n---\nlead\n");
    let prepared = read_agents_folder(&dirs.config);
    assert_eq!(prepared.dir, agents);
    assert!(!prepared.seed && !prepared.shipped);
    assert_eq!(files_in(&roles), before);
}

#[test]
fn a_read_only_host_with_neither_folder_serves_the_shipped_agents_and_writes_none() {
    let dirs = TestDirs::new();
    let prepared = read_agents_folder(&dirs.config);
    assert!(!prepared.seed && prepared.shipped);
    let agents = FolderDefinitions::prepared(prepared);
    let listing = agents.listing();
    let names: Vec<&str> = listing
        .agents
        .iter()
        .map(|agent| agent.name.as_str())
        .collect();
    assert_eq!(names, ["lead", "reviewer", "worker"]);
    assert!(listing.bad.is_empty());
    let DefinitionLookup::Found(reviewer) = agents.lookup("Reviewer") else {
        panic!("the shipped reviewer");
    };
    assert_eq!(*reviewer, shipped("reviewer"));
    assert_eq!(agents.lookup("default"), DefinitionLookup::Missing);
    assert!(
        std::fs::read_dir(&dirs.config).map_or(true, |mut entries| entries.next().is_none()),
        "nothing written in the config folder"
    );

    // A folder made while it runs is read from then on.
    write(&dirs.config.join("agents"), "mine.md", "---\n---\nmine\n");
    let names: Vec<String> = agents
        .listing()
        .agents
        .into_iter()
        .map(|agent| agent.name)
        .collect();
    assert_eq!(names, ["mine"]);
}
