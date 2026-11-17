use std::fs;
use std::path::{MAIN_SEPARATOR, Path};

use floti_protocol::channels::workspace::{CreateWorkspaceRequest, WorkspaceMode};

use super::{WorkspaceCreated, create_workspace, workspace_name_problems};
use crate::testkit::{TestDirs, link_folder};
use crate::workspace::discovery::scan::scan;
use crate::workspace::templates::testing::{
    World, line_matches, looks_like_iso, manifest_value, read, tree, write,
};

// ---------------------------------------------------------------------------
// createWorkspace from a template
// ---------------------------------------------------------------------------

#[test]
fn writes_the_tree_substitutes_only_tpl_and_records_the_provenance() {
    let world = World::new();
    let literal = "runs-on: ${{ matrix.os }}\nname: {{NAME}}\n";
    world.plant(
        "demo",
        &[
            ("template.yaml", "label: \"Demo\"\n"),
            (
                "CLAUDE.md.tpl",
                "# {{NAME}}\n\nfrom {{TEMPLATE}} at {{CREATED_AT}}\n",
            ),
            ("workflow.yml", literal),
            ("dot-claude/settings.json", "{\"model\":\"opus\"}"),
            ("notes/.gitkeep", ""),
        ],
    );

    let result = world.workspace("work", Some("demo"));
    let workspace = world.root.join("work");

    assert!(result.problems.is_empty(), "{:?}", result.problems);
    assert_eq!(result.path.as_deref(), Some(workspace.as_path()));
    assert_eq!(
        tree(&workspace),
        [
            ".claude",
            ".claude/settings.json",
            "CLAUDE.md",
            "notes",
            "workflow.yml",
            "workspace.yaml"
        ]
    );

    // The .tpl was filled in and lost its extension.
    let claude_md = read(&workspace.join("CLAUDE.md"));
    assert!(line_matches(&claude_md, "# work"), "{claude_md}");
    let from = claude_md
        .lines()
        .find_map(|line| line.strip_prefix("from demo at "))
        .unwrap();
    assert!(looks_like_iso(from), "{from}");

    // The non-.tpl file arrived byte for byte, `{{...}}` and all.
    assert_eq!(read(&workspace.join("workflow.yml")), literal);

    // Provenance, and the timestamp the .tpl was given is the manifest's.
    let manifest = read(&workspace.join("workspace.yaml"));
    assert!(line_matches(&manifest, "template: \"demo\""), "{manifest}");
    let created = manifest_value(&manifest, "created").unwrap();
    assert!(claude_md.contains(created), "{claude_md} / {created}");
}

#[test]
fn fills_path_with_the_workspace_folder_forward_slashed_so_a_json_tpl_stays_json() {
    let world = World::new();
    // A parent with a space in it: a workspace under a profile folder with a
    // space is ordinary and the value must survive it unescaped.
    let parent = world.root.join("with space");
    fs::create_dir_all(&parent).unwrap();
    world.plant(
        "wired",
        &[
            ("PATH.txt.tpl", "{{PATH}}"),
            (
                "dot-claude/settings.json.tpl",
                "{\"hooks\":{\"PostToolUse\":[{\"matcher\":\"Bash\",\"hooks\":[{\"type\":\"command\",\"command\":\"node {{PATH}}/.claude/hooks/post-bash.mjs\"}]}]},\"autoMemoryDirectory\":\"{{PATH}}/memory\"}\n",
            ),
        ],
    );

    let result = world.workspace_in(&parent, "my work", Some("wired"));
    let workspace = parent.join("my work");
    let expected = workspace.display().to_string().replace(MAIN_SEPARATOR, "/");

    assert!(result.problems.is_empty(), "{:?}", result.problems);
    assert!(!expected.contains('\\'));
    assert!(expected.contains("with space/my work"));
    assert_eq!(read(&workspace.join("PATH.txt")), expected);

    // The point of the forward slashes: a backslashed Windows path inside a
    // JSON string is an invalid escape, so this parse is what fails without them.
    let settings: serde_json::Value =
        serde_json::from_str(&read(&workspace.join(".claude").join("settings.json"))).unwrap();
    assert_eq!(
        settings["autoMemoryDirectory"],
        format!("{expected}/memory")
    );
    assert_eq!(
        settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
        format!("node {expected}/.claude/hooks/post-bash.mjs")
    );
}

#[test]
fn does_not_write_the_gitkeep_it_made_the_folder_for() {
    let world = World::new();
    world.plant("demo", &[("notes/.gitkeep", "")]);
    world.workspace("work", Some("demo"));

    assert_eq!(tree(&world.root.join("work")), ["notes", "workspace.yaml"]);
}

#[test]
fn refuses_a_template_name_that_is_a_path_and_writes_nothing() {
    let world = World::new();
    for template in ["../escape", "..\\escape", "C:\\Windows\\System32"] {
        let result = world.workspace("work", Some(template));
        assert_eq!(result.path, None, "{template}");
        assert!(!result.problems.is_empty(), "{template}");
        assert_eq!(tree(&world.root), ["templates"], "{template}");
    }
}

#[test]
fn refuses_a_template_that_is_not_there_and_leaves_no_folder_behind() {
    let world = World::new();
    let result = world.workspace("work", Some("absent"));
    assert_eq!(result.path, None);
    assert_eq!(result.problems, ["There is no template called \"absent\"."]);
    assert_eq!(tree(&world.root), ["templates"]);
}

#[test]
fn will_not_follow_a_link_out_of_the_template() {
    let world = World::new();
    let outside = world.root.join("outside");
    write(&outside.join("secret.txt"), "untouched");
    world.plant(
        "hostile",
        &[
            ("template.yaml", "label: \"Hostile\"\n"),
            ("ok.txt", "fine"),
        ],
    );
    link_folder(&outside, &world.templates.join("hostile").join("escape"));

    let result = world.workspace("work", Some("hostile"));

    assert_eq!(
        result.path.as_deref(),
        Some(world.root.join("work").as_path())
    );
    let problems = result.problems.join(" ");
    assert!(problems.contains("escape"), "{problems}");
    assert!(problems.contains("link"), "{problems}");
    // The workspace has the honest half of the template and nothing else, and
    // what the link pointed at is exactly as it was.
    assert_eq!(tree(&world.root.join("work")), ["ok.txt", "workspace.yaml"]);
    assert_eq!(tree(&outside), ["secret.txt"]);
    assert_eq!(read(&outside.join("secret.txt")), "untouched");
}

#[test]
fn will_not_let_a_template_supply_the_manifest() {
    let world = World::new();
    world.plant(
        "demo",
        &[
            ("workspace.yaml.tpl", "name: \"not-yours\"\n"),
            ("a.txt", "x"),
        ],
    );

    let result = world.workspace("work", Some("demo"));

    assert!(
        result.problems.join(" ").contains("workspace.yaml"),
        "{:?}",
        result.problems
    );
    let manifest = read(&world.root.join("work").join("workspace.yaml"));
    assert!(line_matches(&manifest, "name: \"work\""), "{manifest}");
}

#[test]
fn will_not_let_a_template_supply_a_harness_yaml_beside_the_manifest() {
    let world = World::new();
    world.plant(
        "demo",
        &[("harness.yaml", "name: \"not-yours\"\n"), ("a.txt", "x")],
    );

    let result = world.workspace("work", Some("demo"));

    assert!(
        result.problems.join(" ").contains("harness.yaml"),
        "{:?}",
        result.problems
    );
    assert!(!world.root.join("work").join("harness.yaml").exists());
    let manifest = read(&world.root.join("work").join("workspace.yaml"));
    assert!(line_matches(&manifest, "name: \"work\""), "{manifest}");
}

#[test]
fn reports_what_it_actually_wrote_when_one_entry_fails_halfway() {
    let world = World::new();
    // Two sources, one target: `a.txt` and `a.txt.tpl` both want `a.txt`. The
    // first wins, the second is refused, and neither is silent.
    world.plant(
        "demo",
        &[
            ("a.txt", "plain"),
            ("a.txt.tpl", "{{NAME}}"),
            ("b.txt", "also written"),
        ],
    );

    let result = world.workspace("work", Some("demo"));

    assert_eq!(
        result.path.as_deref(),
        Some(world.root.join("work").as_path())
    );
    assert_eq!(result.created, ["a.txt", "b.txt", "workspace.yaml"]);
    assert_eq!(result.problems.len(), 1);
    assert!(
        result.problems[0].contains("a.txt"),
        "{:?}",
        result.problems
    );
    assert_eq!(read(&world.root.join("work").join("a.txt")), "plain");
    assert_eq!(read(&world.root.join("work").join("b.txt")), "also written");
}

#[test]
fn ignores_a_template_when_converting() {
    let world = World::new();
    world.plant("demo", &[("CLAUDE.md.tpl", "# {{NAME}}\n")]);
    fs::create_dir_all(world.root.join("alpha")).unwrap();

    let result = create_workspace(
        &CreateWorkspaceRequest {
            mode: WorkspaceMode::Convert,
            dir: world.root.display().to_string(),
            name: None,
            template: Some("demo".into()),
        },
        Some(&world.templates),
    )
    .unwrap();

    assert!(result.problems.is_empty(), "{:?}", result.problems);
    assert_eq!(result.created, ["workspace.yaml"]);
    let manifest = read(&world.root.join("workspace.yaml"));
    assert!(
        !manifest.lines().any(|line| line.starts_with("template:")),
        "{manifest}"
    );
    assert!(!tree(&world.root).contains(&"CLAUDE.md".to_owned()));
}

// ---------------------------------------------------------------------------
// createWorkspace
// ---------------------------------------------------------------------------

fn new_workspace(dir: &Path, name: &str, template: Option<&str>) -> WorkspaceCreated {
    create_workspace(
        &CreateWorkspaceRequest {
            mode: WorkspaceMode::New,
            dir: dir.display().to_string(),
            name: Some(name.into()),
            template: template.map(str::to_owned),
        },
        None,
    )
    .unwrap()
}

fn convert(dir: &Path) -> WorkspaceCreated {
    create_workspace(
        &CreateWorkspaceRequest {
            mode: WorkspaceMode::Convert,
            dir: dir.display().to_string(),
            name: None,
            template: None,
        },
        None,
    )
    .unwrap()
}

fn project_names(roots: &[&Path]) -> (Vec<String>, Vec<String>) {
    let roots: Vec<String> = roots
        .iter()
        .map(|root| root.display().to_string())
        .collect();
    let result = scan(&roots, false);
    let mut projects: Vec<String> = result.projects.into_iter().map(|p| p.name).collect();
    projects.sort();
    (
        result.workspaces.into_iter().map(|h| h.name).collect(),
        projects,
    )
}

/// Starter's AGENTS.md for a workspace called "My Work", as the onboarding
/// design draws it. Spelt out rather than read from the embedded file, so a
/// change to the text is a change to this test too.
const STARTER_AGENTS_MD_FOR_MY_WORK: &str = "# My Work

A Floti workspace: a folder that holds repositories and the instructions every
agent working on them follows. Floti gives this file to each session started
here or in a repository under it, whichever agent runs it.

## Layout

- `workspace.yaml` is Floti's record of this workspace.
- `repos/` holds the repositories, one folder each. Clone new ones here.

## How to work here

- Work in the repository the task is about, and read its own instructions
  (its AGENTS.md or CLAUDE.md) first. They win over this file inside it.
- Make changes on a branch, not the default branch.
- Ask before anything hard to undo or seen outside this computer: pushing,
  merging, publishing, deleting or sending.
- Run the tests for what you changed before calling it done, and say what
  you did not check.
- Keep to what was asked. Mention anything else you notice instead of
  fixing it unasked.

## This file

Edit it to say what agents here should know: what the repositories are for,
how they fit together, and the rules that hold across them. A skill every
agent should have goes in `.agents/skills/<name>/SKILL.md`.
";

#[test]
fn starter_writes_the_manifest_an_empty_repos_and_agents_md_with_its_name_and_nothing_else() {
    let dirs = TestDirs::new();
    let root = dirs.folder("workspace root");
    let result = new_workspace(&root, "My Work", Some("starter"));
    let workspace = root.join("My Work");

    assert!(result.problems.is_empty(), "{:?}", result.problems);
    assert_eq!(result.path.as_deref(), Some(workspace.as_path()));
    assert_eq!(result.created, ["repos", "AGENTS.md", "workspace.yaml"]);
    assert_eq!(tree(&workspace), ["AGENTS.md", "repos", "workspace.yaml"]);
    assert_eq!(
        read(&workspace.join("AGENTS.md")),
        STARTER_AGENTS_MD_FOR_MY_WORK
    );
    assert!(line_matches(
        &read(&workspace.join("workspace.yaml")),
        "template: \"starter\""
    ));
}

#[test]
fn makes_starter_when_no_template_is_named_or_the_old_built_in_is() {
    for asked in [None, Some(""), Some("minimal"), Some("Starter")] {
        let dirs = TestDirs::new();
        let root = dirs.folder("workspace root");
        let result = new_workspace(&root, "work", asked);

        assert!(
            result.problems.is_empty(),
            "{asked:?}: {:?}",
            result.problems
        );
        assert_eq!(
            tree(&root.join("work")),
            ["AGENTS.md", "repos", "workspace.yaml"],
            "{asked:?}"
        );
        assert!(
            line_matches(
                &read(&root.join("work").join("workspace.yaml")),
                "template: \"starter\""
            ),
            "{asked:?}"
        );
    }
}

#[test]
fn writes_four_keys_and_no_prose() {
    let dirs = TestDirs::new();
    let root = dirs.folder("workspace root");
    new_workspace(&root, "work", Some("starter"));
    let manifest = read(&root.join("work").join("workspace.yaml"));

    let keys: Vec<&str> = manifest
        .trim()
        .lines()
        .map(|line| line.split(':').next().unwrap())
        .collect();
    assert_eq!(keys, ["name", "template", "version", "created"]);
    assert!(!manifest.contains('#'));
    assert!(line_matches(&manifest, "name: \"work\""));
    assert!(line_matches(&manifest, "template: \"starter\""));
    assert!(looks_like_iso(
        manifest_value(&manifest, "created").unwrap()
    ));
}

#[test]
fn produces_a_workspace_the_scanner_immediately_sees() {
    let dirs = TestDirs::new();
    let root = dirs.folder("workspace root");
    let path = new_workspace(&root, "work", None).path.unwrap();
    fs::create_dir_all(path.join("repos").join("alpha")).unwrap();

    let (workspaces, projects) = project_names(&[&path]);

    assert_eq!(workspaces, ["work"]);
    assert_eq!(projects, ["alpha", "work"]);
}

#[test]
fn accepts_a_name_with_spaces_and_dashes() {
    let dirs = TestDirs::new();
    let root = dirs.folder("workspace root");
    let result = new_workspace(&root, "My Work-Space", None);
    assert!(result.problems.is_empty(), "{:?}", result.problems);
    assert_eq!(
        result.path.as_deref(),
        Some(root.join("My Work-Space").as_path())
    );
}

#[test]
fn refuses_a_name_that_is_not_a_folder_name() {
    let dirs = TestDirs::new();
    let root = dirs.folder("workspace root");
    for name in ["", "  ", "a/b", "a\\b", "a:b", "con", "trailing.", ".."] {
        let result = new_workspace(&root, name, None);
        assert_eq!(result.path, None, "{name:?}");
        assert!(!result.problems.is_empty(), "{name:?}");
    }
}

#[test]
fn refuses_to_write_into_a_folder_that_already_exists() {
    let dirs = TestDirs::new();
    let root = dirs.folder("workspace root");
    write(&root.join("work").join("mine.txt"), "mine");
    let result = new_workspace(&root, "work", None);
    assert_eq!(result.path, None);
    assert_eq!(
        result.problems,
        [format!("{} already exists.", root.join("work").display())]
    );
    assert_eq!(tree(&root.join("work")), ["mine.txt"]);
    assert_eq!(read(&root.join("work").join("mine.txt")), "mine");
}

#[test]
fn converts_a_folder_of_repos_in_place_without_hiding_them() {
    let dirs = TestDirs::new();
    let root = dirs.folder("workspace root");
    fs::create_dir_all(root.join("alpha")).unwrap();
    fs::create_dir_all(root.join("beta")).unwrap();

    let result = convert(&root);
    assert!(result.problems.is_empty(), "{:?}", result.problems);
    assert_eq!(result.created, ["workspace.yaml"]);
    assert_eq!(tree(&root), ["alpha", "beta", "workspace.yaml"]);

    let manifest = read(&root.join("workspace.yaml"));
    assert!(line_matches(&manifest, "repos: \".\""), "{manifest}");
    let keys: Vec<&str> = manifest
        .trim()
        .lines()
        .map(|line| line.split(':').next().unwrap())
        .collect();
    assert_eq!(keys, ["name", "version", "created", "repos"]);

    let (workspaces, projects) = project_names(&[&root]);
    assert_eq!(workspaces.len(), 1);
    let mut expected = vec!["alpha".to_owned(), "beta".to_owned(), workspaces[0].clone()];
    expected.sort();
    assert_eq!(projects, expected);
}

#[test]
fn omits_repos_when_converting_a_folder_that_already_has_repos() {
    let dirs = TestDirs::new();
    let root = dirs.folder("workspace root");
    fs::create_dir_all(root.join("repos").join("alpha")).unwrap();

    convert(&root);

    let manifest = read(&root.join("workspace.yaml"));
    assert!(
        !manifest.lines().any(|line| line.starts_with("repos:")),
        "{manifest}"
    );
    let (_, projects) = project_names(&[&root]);
    assert!(projects.contains(&"alpha".to_owned()), "{projects:?}");
}

#[test]
fn converting_leaves_the_folders_own_instructions_and_config_as_they_are() {
    let dirs = TestDirs::new();
    let root = dirs.folder("workspace root");
    write(&root.join("CLAUDE.md"), "# mine\n");
    write(&root.join(".claude").join("settings.json"), "{}");
    fs::create_dir_all(root.join("repos").join("alpha")).unwrap();

    let result = convert(&root);

    assert_eq!(result.created, ["workspace.yaml"]);
    assert_eq!(
        tree(&root),
        [
            ".claude",
            ".claude/settings.json",
            "CLAUDE.md",
            "repos",
            "repos/alpha",
            "workspace.yaml"
        ]
    );
    assert_eq!(read(&root.join("CLAUDE.md")), "# mine\n");
    assert!(!root.join("AGENTS.md").exists());
}

#[test]
fn a_workspace_recorded_as_minimal_still_lists_with_its_label_and_its_repositories() {
    let dirs = TestDirs::new();
    let root = dirs.folder("older");
    write(
        &root.join("workspace.yaml"),
        "name: \"older\"\ntemplate: \"minimal\"\nversion: \"1\"\ncreated: \"2026-01-01T00:00:00.000Z\"\n",
    );
    fs::create_dir_all(root.join(".claude")).unwrap();
    fs::create_dir_all(root.join("repos").join("alpha")).unwrap();

    let result = scan(&[root.display().to_string()], false);

    assert_eq!(result.workspaces.len(), 1);
    assert_eq!(result.workspaces[0].name, "older");
    assert_eq!(result.workspaces[0].template.as_deref(), Some("minimal"));
    let mut projects: Vec<String> = result.projects.into_iter().map(|p| p.name).collect();
    projects.sort();
    assert_eq!(projects, ["alpha", "older"]);
}

#[test]
fn refuses_to_convert_a_folder_that_is_already_a_workspace() {
    let dirs = TestDirs::new();
    let root = dirs.folder("workspace root");
    fs::write(root.join("workspace.yaml"), "name: \"already\"\n").unwrap();
    let result = convert(&root);
    assert_eq!(result.path, None);
    assert_eq!(
        result.problems,
        [format!("{} is already a workspace.", root.display())]
    );
}

#[test]
fn refuses_to_convert_a_folder_with_a_harness_yaml_and_leaves_it_as_it_is() {
    let dirs = TestDirs::new();
    let root = dirs.folder("older workspace");
    fs::write(root.join("harness.yaml"), "name: \"older\"\n").unwrap();
    let result = convert(&root);
    assert_eq!(result.path, None);
    assert_eq!(
        result.problems,
        [format!("{} is already a workspace.", root.display())]
    );
    assert_eq!(tree(&root), ["harness.yaml"]);
    assert_eq!(read(&root.join("harness.yaml")), "name: \"older\"\n");
}

#[test]
fn refuses_a_new_workspace_over_a_folder_with_a_harness_yaml() {
    let dirs = TestDirs::new();
    let parent = dirs.folder("parent");
    fs::create_dir_all(parent.join("older")).unwrap();
    fs::write(parent.join("older").join("harness.yaml"), "name: older\n").unwrap();
    let result = new_workspace(&parent, "older", None);
    assert_eq!(result.path, None);
    assert_eq!(
        result.problems,
        [format!(
            "{} is already a workspace.",
            parent.join("older").display()
        )]
    );
    assert_eq!(tree(&parent.join("older")), ["harness.yaml"]);
}

#[test]
fn passes_ordinary_workspace_names() {
    for name in ["dev", "my-work", "Work Space", "a1"] {
        assert!(workspace_name_problems(name).is_empty(), "{name}");
    }
}
