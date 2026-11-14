use std::fs;
use std::path::{Path, PathBuf};

use floti_protocol::channels::workspace::{
    CreateTemplateRequest, FolderTemplateKind, RenameTemplateRequest, SaveFolderAsTemplateRequest,
    TemplateFileRequest, TemplateMetadataRequest,
};

use super::{
    ImportFile, create_template, delete_template, import_into_template, make_substitutable,
    preview_folder_as_template, read_template_detail, rename_template, save_folder_as_template,
    template_name_problems, write_template_metadata,
};
use crate::agent_config::snapshots;
use crate::platform::links;
use crate::testkit::link_folder;
use crate::workspace::templates::reader::{list_templates, template_id_problems};
use crate::workspace::templates::testing::{World, read, tree, write, write_bytes};

// ---------------------------------------------------------------------------
// Naming
// ---------------------------------------------------------------------------

#[test]
fn refuses_a_path_an_absolute_the_built_in_name_and_a_name_already_taken() {
    let world = World::new();
    let dir = &world.templates;
    assert_eq!(template_name_problems(dir, "", false).len(), 1);
    assert_eq!(template_name_problems(dir, "../escape", false).len(), 1);
    assert!(!template_name_problems(dir, "C:\\Windows", false).is_empty());
    assert!(template_name_problems(dir, "minimal", false)[0].contains("built-in"));
    // Case-folded, because Windows would let a `Minimal` folder shadow the row
    // that always works and the reader refuses only the exact name.
    assert!(template_name_problems(dir, "MINIMAL", false)[0].contains("built-in"));

    fs::create_dir(dir.join("taken")).unwrap();
    assert!(template_name_problems(dir, "taken", false)[0].contains("already a template"));
    assert!(template_name_problems(dir, "taken", true).is_empty());
}

#[test]
fn accepts_an_ordinary_template_name() {
    let world = World::new();
    assert!(template_name_problems(&world.templates, "client-work", false).is_empty());
}

// ---------------------------------------------------------------------------
// Create, metadata, rename, delete
// ---------------------------------------------------------------------------

#[test]
fn scaffolds_a_manifest_and_an_empty_tree_and_the_picker_then_offers_it() {
    let world = World::new();
    let result = create_template(
        world.conn(),
        &world.templates,
        &CreateTemplateRequest {
            name: "client-work".into(),
            label: Some("Client work".into()),
            description: Some("One sentence.".into()),
        },
    );

    assert!(result.ok, "{:?}", result.problems);
    assert_eq!(result.template.as_deref(), Some("client-work"));
    assert_eq!(
        tree(&world.templates.join("client-work")),
        ["template.yaml"]
    );

    let listing = list_templates(&world.templates);
    assert_eq!(world.listed_ids(), ["minimal", "client-work"]);
    assert_eq!(listing.templates[1].label, "Client work");
    assert_eq!(
        listing.templates[1].description.as_deref(),
        Some("One sentence.")
    );
}

#[test]
fn uses_the_folder_name_when_no_label_is_given_and_writes_nothing_twice() {
    let world = World::new();
    world.create("bare", None);
    assert!(read(&world.templates.join("bare").join("template.yaml")).contains("\"bare\""));

    let again = create_template(
        world.conn(),
        &world.templates,
        &CreateTemplateRequest {
            name: "bare".into(),
            label: None,
            description: None,
        },
    );
    assert!(!again.ok);
    assert!(
        again.problems[0].contains("already a template"),
        "{:?}",
        again.problems
    );
}

#[test]
fn refuses_to_create_a_template_whose_name_is_a_path_and_writes_nothing() {
    let world = World::new();
    assert!(!world.create("../escape", None));
    assert!(tree(&world.templates).is_empty());
}

#[test]
fn records_the_manifest_in_the_snapshot_table_because_it_goes_through_the_snapshot_first_writer() {
    let world = World::new();
    let rows: i64 = world
        .conn()
        .query_row("SELECT count(*) FROM config_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0);

    world.create("recorded", Some("Recorded"));
    let dir = world.templates.join("recorded").display().to_string();
    let reasons = |world: &World| -> Vec<String> {
        snapshots::list(world.conn(), &dir, "template.yaml")
            .unwrap()
            .into_iter()
            .map(|row| {
                serde_json::to_value(row.reason)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    };
    assert_eq!(reasons(&world), ["create"]);

    write_template_metadata(
        world.conn(),
        &world.templates,
        &TemplateMetadataRequest {
            template: "recorded".into(),
            label: "Renamed".into(),
            description: String::new(),
        },
    );
    // Newest first, and the edit's row holds what was there before it.
    assert_eq!(reasons(&world), ["edit", "create"]);
}

#[test]
fn rewrites_label_and_description_without_touching_the_rest_of_the_folder() {
    let world = World::new();
    world.create("work", Some("Work"));
    write(
        &world.templates.join("work").join("CLAUDE.md.tpl"),
        "# {{NAME}}\n",
    );

    let result = write_template_metadata(
        world.conn(),
        &world.templates,
        &TemplateMetadataRequest {
            template: "work".into(),
            label: "Renamed label".into(),
            description: "A new sentence.".into(),
        },
    );

    assert!(result.ok, "{:?}", result.problems);
    let listing = list_templates(&world.templates);
    assert_eq!(listing.templates[1].label, "Renamed label");
    assert_eq!(
        listing.templates[1].description.as_deref(),
        Some("A new sentence.")
    );
    assert_eq!(
        read(&world.templates.join("work").join("CLAUDE.md.tpl")),
        "# {{NAME}}\n"
    );
}

#[test]
fn quotes_a_label_containing_a_colon_which_bare_yaml_would_not_survive() {
    let world = World::new();
    world.create("work", None);
    write_template_metadata(
        world.conn(),
        &world.templates,
        &TemplateMetadataRequest {
            template: "work".into(),
            label: "Work: the sequel".into(),
            description: String::new(),
        },
    );

    let listing = list_templates(&world.templates);
    assert!(listing.problems.is_empty(), "{:?}", listing.problems);
    assert_eq!(listing.templates[1].label, "Work: the sequel");
}

#[test]
fn refuses_metadata_for_a_template_that_is_not_there() {
    let world = World::new();
    let result = write_template_metadata(
        world.conn(),
        &world.templates,
        &TemplateMetadataRequest {
            template: "nothing".into(),
            label: "x".into(),
            description: String::new(),
        },
    );
    assert!(!result.ok);
    assert!(
        result.problems[0].contains("no template called"),
        "{:?}",
        result.problems
    );
}

fn rename(world: &World, template: &str, name: &str) -> bool {
    rename_template(
        &world.templates,
        &RenameTemplateRequest {
            template: template.into(),
            name: name.into(),
        },
    )
    .ok
}

#[test]
fn moves_the_whole_folder_and_keeps_its_contents() {
    let world = World::new();
    world.create("before", Some("Before"));
    write(
        &world.templates.join("before").join("notes").join("a.md"),
        "hello",
    );

    assert!(rename(&world, "before", "after"));
    assert!(!world.templates.join("before").exists());
    assert_eq!(
        read(&world.templates.join("after").join("notes").join("a.md")),
        "hello"
    );
}

#[test]
fn refuses_a_rename_to_a_name_that_is_taken_the_built_in_name_and_its_own_name() {
    let world = World::new();
    world.create("a", None);
    world.create("b", None);

    assert!(!rename(&world, "a", "b"));
    assert!(!rename(&world, "a", "minimal"));
    assert!(!rename(&world, "a", "a"));
    assert!(world.templates.join("a").exists());
}

#[test]
fn removes_the_folder_and_everything_in_it() {
    let world = World::new();
    world.create("gone", None);
    write(
        &world
            .templates
            .join("gone")
            .join("deep")
            .join("nested")
            .join("a.txt"),
        "x",
    );

    let result = delete_template(&world.templates, "gone");

    assert!(result.ok, "{:?}", result.problems);
    assert!(!world.templates.join("gone").exists());
    assert!(tree(&world.templates).is_empty());
}

#[test]
fn unlinks_a_link_rather_than_walking_into_what_it_points_at() {
    let world = World::new();
    let canary = world.root.join("canary");
    write(&canary.join("do-not-touch.txt"), "untouched");
    world.create("hostile", None);
    link_folder(&canary, &world.templates.join("hostile").join("escape"));

    let result = delete_template(&world.templates, "hostile");

    assert!(result.ok, "{:?}", result.problems);
    assert!(!world.templates.join("hostile").exists());
    // The claim is about the bytes, not about a filename being absent.
    assert_eq!(read(&canary.join("do-not-touch.txt")), "untouched");
}

#[test]
fn refuses_a_delete_whose_name_climbs_out_and_removes_nothing() {
    let world = World::new();
    fs::create_dir_all(world.root.join("sibling")).unwrap();
    let result = delete_template(&world.templates, "../sibling");
    assert!(!result.ok);
    assert!(world.root.join("sibling").exists());
}

#[test]
fn refuses_to_delete_a_template_that_is_itself_a_link_and_leaves_its_target() {
    let world = World::new();
    let canary = world.root.join("canary");
    write(&canary.join("do-not-touch.txt"), "untouched");
    link_folder(&canary, &world.templates.join("linked"));

    let result = delete_template(&world.templates, "linked");

    assert!(!result.ok);
    assert!(links::is_link(&world.templates.join("linked")));
    assert_eq!(read(&canary.join("do-not-touch.txt")), "untouched");
}

// ---------------------------------------------------------------------------
// Detail and `.tpl`
// ---------------------------------------------------------------------------

#[test]
fn lists_every_file_with_what_a_harness_would_receive_it_as() {
    let world = World::new();
    world.create("shaped", Some("Shaped"));
    let dir = world.templates.join("shaped");
    write(&dir.join("CLAUDE.md.tpl"), "# {{NAME}}\n");
    write(&dir.join("workflow.yml"), "on: push\n");
    write(&dir.join("dot-claude").join("settings.json"), "{}\n");
    write(&dir.join("notes").join(".gitkeep"), "");

    let detail = read_template_detail(&world.templates, "shaped");

    assert_eq!(detail.label, "Shaped");
    assert_eq!(detail.file_count, 4);
    let mapped: Vec<String> = detail
        .files
        .iter()
        .map(|file| format!("{} -> {}", file.rel_path, file.target))
        .collect();
    assert_eq!(
        mapped,
        [
            "CLAUDE.md.tpl -> CLAUDE.md",
            "dot-claude/settings.json -> .claude/settings.json",
            "notes/.gitkeep -> notes/",
            "workflow.yml -> workflow.yml"
        ]
    );
    let substituted: Vec<&str> = detail
        .files
        .iter()
        .filter(|file| file.substituted)
        .map(|file| file.rel_path.as_str())
        .collect();
    assert_eq!(substituted, ["CLAUDE.md.tpl"]);
    // `template.yaml` describes the template rather than being content of it.
    assert!(
        !detail
            .files
            .iter()
            .any(|file| file.rel_path == "template.yaml")
    );
}

#[test]
fn names_a_link_rather_than_leaving_it_out_of_the_list() {
    let world = World::new();
    let canary = world.root.join("canary");
    write(&canary.join("inside.txt"), "untouched");
    world.create("linky", None);
    link_folder(&canary, &world.templates.join("linky").join("escape"));

    let detail = read_template_detail(&world.templates, "linky");

    let listed: Vec<&str> = detail
        .files
        .iter()
        .map(|file| file.rel_path.as_str())
        .collect();
    assert_eq!(listed, ["escape"]);
    assert!(detail.files[0].link);
    assert!(
        detail.problems.join(" ").contains("escape"),
        "{:?}",
        detail.problems
    );
    // A link is not a file the template contributes.
    assert_eq!(detail.file_count, 0);
}

#[test]
fn says_which_template_is_not_there_rather_than_reporting_an_empty_one() {
    let world = World::new();
    let detail = read_template_detail(&world.templates, "nothing");
    assert!(
        detail.problems[0].contains("no template called"),
        "{:?}",
        detail.problems
    );
}

fn substitute(world: &World, template: &str, path: &str) -> (bool, Vec<String>) {
    let result = make_substitutable(
        &world.templates,
        &TemplateFileRequest {
            template: template.into(),
            path: path.into(),
        },
    );
    (result.ok, result.problems)
}

#[test]
fn renames_a_file_to_tpl_and_the_engine_then_substitutes_it() {
    let world = World::new();
    world.create("sub", None);
    write(
        &world.templates.join("sub").join("CLAUDE.md"),
        "# {{NAME}}\n",
    );

    let (ok, problems) = substitute(&world, "sub", "CLAUDE.md");

    assert!(ok, "{problems:?}");
    assert!(!world.templates.join("sub").join("CLAUDE.md").exists());
    let created = world.harness("made", Some("sub"));
    assert!(created.problems.is_empty(), "{:?}", created.problems);
    assert_eq!(read(&world.root.join("made").join("CLAUDE.md")), "# made\n");
}

#[test]
fn refuses_a_binary_a_file_already_substituted_and_the_manifest() {
    let world = World::new();
    world.create("sub", None);
    let dir = world.templates.join("sub");
    write_bytes(&dir.join("logo.png"), &[0x89, 0x50, 0x00, 0x01, 0x02]);
    write(&dir.join("a.md.tpl"), "x");

    let (_, problems) = substitute(&world, "sub", "logo.png");
    assert!(problems[0].contains("not text"), "{problems:?}");
    assert!(!substitute(&world, "sub", "a.md.tpl").0);
    assert!(!substitute(&world, "sub", "template.yaml").0);
    assert!(dir.join("logo.png").exists());
}

#[test]
fn refuses_a_substitution_path_that_climbs_out_of_the_template() {
    let world = World::new();
    world.create("sub", None);
    write(&world.root.join("outside.md"), "x");
    assert!(!substitute(&world, "sub", "../../outside.md").0);
    assert!(world.root.join("outside.md").exists());
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

#[test]
fn copies_files_in_as_plain_bytes_and_reports_created_against_replaced() {
    let world = World::new();
    world.create("target", None);
    let source = write(
        &world.root.join("project/.claude/skills/think/SKILL.md"),
        "body\n",
    );
    let files = [ImportFile {
        source: source.clone(),
        target: ".claude/skills/think/SKILL.md".into(),
    }];
    let copied = world.templates.join("target/.claude/skills/think/SKILL.md");

    let first = import_into_template(&world.templates, "target", &files);
    assert_eq!(first.created, [".claude/skills/think/SKILL.md"]);
    assert!(first.replaced.is_empty());
    assert_eq!(read(&copied), "body\n");

    fs::write(&source, "edited\n").unwrap();
    let second = import_into_template(&world.templates, "target", &files);
    assert!(second.created.is_empty());
    assert_eq!(second.replaced, [".claude/skills/think/SKILL.md"]);
    assert_eq!(read(&copied), "edited\n");
}

#[test]
fn copies_bytes_rather_than_text_so_a_bundled_binary_survives() {
    let world = World::new();
    world.create("target", None);
    let bytes = [0x00, 0xff, 0x10, 0x89, 0x50];
    let source = write_bytes(&world.root.join("logo.png"), &bytes);

    import_into_template(
        &world.templates,
        "target",
        &[ImportFile {
            source,
            target: ".claude/skills/think/logo.png".into(),
        }],
    );

    assert_eq!(
        fs::read(world.templates.join("target/.claude/skills/think/logo.png")).unwrap(),
        bytes
    );
}

#[test]
fn refuses_a_source_that_is_a_link_rather_than_copying_what_it_points_at() {
    let world = World::new();
    world.create("target", None);
    let canary = world.root.join("canary");
    write(&canary.join("do-not-touch.txt"), "untouched");
    let link = world.root.join("linked");
    link_folder(&canary, &link);

    let result = import_into_template(
        &world.templates,
        "target",
        &[ImportFile {
            source: link,
            target: ".claude/skills/think/SKILL.md".into(),
        }],
    );

    assert!(!result.ok);
    assert!(result.created.is_empty());
    assert!(!world.templates.join("target").join(".claude").exists());
    assert_eq!(read(&canary.join("do-not-touch.txt")), "untouched");
}

#[test]
fn refuses_a_target_that_climbs_out_of_the_template_and_writes_nothing_there() {
    let world = World::new();
    world.create("target", None);
    let source = write(&world.root.join("a.md"), "x");

    let result = import_into_template(
        &world.templates,
        "target",
        &[ImportFile {
            source,
            target: "../../escaped.md".into(),
        }],
    );

    assert!(!result.ok);
    assert!(
        result.problems[0].contains("escaped.md"),
        "{:?}",
        result.problems
    );
    assert!(!world.root.join("escaped.md").exists());
}

#[test]
fn refuses_a_target_that_would_write_through_a_link_inside_the_template() {
    let world = World::new();
    world.create("target", None);
    let canary = world.root.join("canary");
    fs::create_dir_all(&canary).unwrap();
    link_folder(&canary, &world.templates.join("target").join("escape"));
    let source = write(&world.root.join("a.md"), "x");

    let result = import_into_template(
        &world.templates,
        "target",
        &[ImportFile {
            source,
            target: "escape/a.md".into(),
        }],
    );

    assert!(!result.ok);
    assert!(result.problems[0].contains("link"), "{:?}", result.problems);
    assert!(tree(&canary).is_empty());
}

// ---------------------------------------------------------------------------
// A folder becomes a template
// ---------------------------------------------------------------------------

/// A harness with the things somebody would want to leave behind.
fn plant_harness(world: &World) -> PathBuf {
    plant_harness_in(&world.root)
}

/// [`plant_harness`] under `parent`.
fn plant_harness_in(parent: &Path) -> PathBuf {
    let dir = parent.join("my-harness");
    write(&dir.join("harness.yaml"), "name: my-harness\n");
    write(&dir.join("CLAUDE.md"), "# my-harness\n");
    write(&dir.join(".claude/skills/think/SKILL.md"), "skill\n");
    write(&dir.join("notes/journal.md"), "a private journal\n");
    write(&dir.join("repos/thing/README.md"), "a checkout\n");
    write(&dir.join(".git/HEAD"), "ref: refs/heads/main\n");
    write(
        &dir.join("tools/node_modules/left-pad/index.js"),
        "module.exports = 1\n",
    );
    write(&dir.join("tools/run.mjs"), "console.log(1)\n");
    dir
}

fn preview(
    dir: &Path,
    kind: FolderTemplateKind,
) -> floti_protocol::channels::workspace::FolderTemplatePreview {
    preview_folder_as_template(&dir.display().to_string(), kind)
}

fn save(
    world: &World,
    dir: &Path,
    name: &str,
    label: &str,
    include: Vec<String>,
) -> floti_protocol::channels::workspace::SaveFolderAsTemplateResult {
    save_folder_as_template(
        world.conn(),
        &world.templates,
        &SaveFolderAsTemplateRequest {
            dir: dir.display().to_string(),
            kind: FolderTemplateKind::Harness,
            name: name.into(),
            label: label.into(),
            description: String::new(),
            include,
        },
    )
}

fn ticked(preview: &floti_protocol::channels::workspace::FolderTemplatePreview) -> Vec<String> {
    preview
        .entries
        .iter()
        .filter(|entry| entry.included)
        .map(|entry| entry.name.clone())
        .collect()
}

#[test]
fn lists_the_top_level_unticks_the_instance_data_and_counts_what_is_ticked() {
    let world = World::new();
    let dir = plant_harness(&world);

    let preview = preview(&dir, FolderTemplateKind::Harness);
    let entry = |name: &str| {
        preview
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
    };

    let mut names: Vec<&str> = preview
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            ".claude",
            ".git",
            "CLAUDE.md",
            "harness.yaml",
            "notes",
            "repos",
            "tools"
        ]
    );
    // The defaults, and each is listed rather than pruned.
    assert!(!entry("harness.yaml").included);
    assert!(!entry(".git").included);
    assert!(!entry("repos").included);
    assert!(entry(".claude").included);
    assert!(entry("notes").included);
    assert!(entry("tools").included);

    // `harness.yaml` and `.git` are refused whatever the user ticks; `repos` is
    // merely unticked, because a repos/ scaffold is a defensible thing to want.
    assert!(entry("harness.yaml").refused.is_some());
    assert!(entry(".git").refused.is_some());
    assert!(entry("repos").refused.is_none());

    // `tools/node_modules` is not counted: it is never copied, at any depth.
    assert_eq!(entry("tools").file_count, 1);
    // Ticked by default: CLAUDE.md, .claude/skills/think/SKILL.md,
    // notes/journal.md, tools/run.mjs.
    assert_eq!(preview.file_count, 4);
    assert!(preview.total_bytes > 0);
}

#[test]
fn names_a_link_at_the_top_level_and_never_follows_one_below_it() {
    let world = World::new();
    let canary = world.root.join("canary");
    write(&canary.join("do-not-touch.txt"), "untouched");
    let dir = plant_harness(&world);
    link_folder(&canary, &dir.join("escape"));
    link_folder(&canary, &dir.join("tools").join("deep-escape"));

    let preview = preview(&dir, FolderTemplateKind::Harness);
    let escape = preview
        .entries
        .iter()
        .find(|entry| entry.name == "escape")
        .unwrap();

    assert!(escape.link);
    assert!(!escape.included);
    let problems = preview.problems.join(" ");
    assert!(problems.contains("escape"), "{problems}");
    assert!(problems.contains("deep-escape"), "{problems}");
    // The link's target contributes nothing to the count.
    let tools = preview
        .entries
        .iter()
        .find(|entry| entry.name == "tools")
        .unwrap();
    assert_eq!(tools.file_count, 1);
}

#[test]
fn copies_what_was_ticked_leaves_out_what_was_not_and_writes_a_manifest() {
    let world = World::new();
    let dir = plant_harness(&world);
    let include = ticked(&preview(&dir, FolderTemplateKind::Harness));

    let result = save_folder_as_template(
        world.conn(),
        &world.templates,
        &SaveFolderAsTemplateRequest {
            dir: dir.display().to_string(),
            kind: FolderTemplateKind::Harness,
            name: "frozen".into(),
            label: "Frozen".into(),
            description: "A harness, frozen.".into(),
            include,
        },
    );

    assert!(result.ok, "{:?}", result.problems);
    assert_eq!(
        tree(&world.templates.join("frozen")),
        [
            ".claude",
            ".claude/skills",
            ".claude/skills/think",
            ".claude/skills/think/SKILL.md",
            "CLAUDE.md",
            "notes",
            "notes/journal.md",
            "template.yaml",
            "tools",
            "tools/run.mjs"
        ]
    );
    // The manifest a harness gets is the host's to write, so it is not carried in.
    assert!(!world.templates.join("frozen").join("harness.yaml").exists());
    assert_eq!(result.file_count, 4);

    let listing = list_templates(&world.templates);
    assert_eq!(world.listed_ids(), ["minimal", "frozen"]);
    assert_eq!(listing.templates[1].label, "Frozen");
}

#[test]
fn refuses_harness_yaml_even_when_it_is_ticked_rather_than_authoring_a_broken_template() {
    let world = World::new();
    let dir = plant_harness(&world);

    let result = save(
        &world,
        &dir,
        "frozen",
        "",
        vec!["harness.yaml".into(), "CLAUDE.md".into()],
    );

    assert!(!world.templates.join("frozen").join("harness.yaml").exists());
    assert!(world.templates.join("frozen").join("CLAUDE.md").exists());
    assert!(
        result.problems.join(" ").contains("harness.yaml"),
        "{:?}",
        result.problems
    );
}

#[test]
fn does_not_recurse_into_a_link_and_what_it_pointed_at_is_untouched() {
    let world = World::new();
    let canary = world.root.join("canary");
    write(&canary.join("do-not-touch.txt"), "untouched");
    let dir = plant_harness(&world);
    link_folder(&canary, &dir.join("tools").join("deep-escape"));

    let result = save(&world, &dir, "frozen", "", vec!["tools".into()]);

    assert!(result.ok, "{:?}", result.problems);
    assert_eq!(
        tree(&world.templates.join("frozen")),
        ["template.yaml", "tools", "tools/run.mjs"]
    );
    assert!(
        result.problems.join(" ").contains("deep-escape"),
        "{:?}",
        result.problems
    );
    assert_eq!(read(&canary.join("do-not-touch.txt")), "untouched");

    assert!(delete_template(&world.templates, "frozen").ok);
    assert_eq!(read(&canary.join("do-not-touch.txt")), "untouched");
    // The folder it was saved from still has its link, unwalked and intact.
    assert!(links::is_link(&dir.join("tools").join("deep-escape")));
}

#[test]
fn keeps_dot_claude_verbatim_and_a_harness_made_from_it_receives_claude() {
    let world = World::new();
    let folder = world.root.join("authored-elsewhere");
    write(&folder.join("dot-claude/skills/think/SKILL.md"), "skill\n");
    write(&folder.join("CLAUDE.md.tpl"), "# {{NAME}}\n");

    let include = ticked(&preview(&folder, FolderTemplateKind::Folder));
    let saved = save_folder_as_template(
        world.conn(),
        &world.templates,
        &SaveFolderAsTemplateRequest {
            dir: folder.display().to_string(),
            kind: FolderTemplateKind::Folder,
            name: "imported".into(),
            label: "Imported".into(),
            description: String::new(),
            include,
        },
    );
    assert!(saved.ok, "{:?}", saved.problems);
    // Verbatim in the template: the alias is applied by the writer rather
    // than undone on the way in.
    assert!(world.templates.join("imported").join("dot-claude").exists());
    assert!(world.listed_ids().contains(&"imported".to_owned()));

    world.harness("from-imported", Some("imported"));
    assert_eq!(
        read(
            &world
                .root
                .join("from-imported/.claude/skills/think/SKILL.md")
        ),
        "skill\n"
    );
    assert_eq!(
        read(&world.root.join("from-imported").join("CLAUDE.md")),
        "# from-imported\n"
    );
}

#[test]
fn refuses_a_name_that_is_taken_and_leaves_the_existing_template_alone() {
    let world = World::new();
    world.create("taken", Some("Taken"));
    let dir = plant_harness(&world);

    let result = save(&world, &dir, "taken", "", vec!["CLAUDE.md".into()]);

    assert!(!result.ok);
    assert_eq!(tree(&world.templates.join("taken")), ["template.yaml"]);
}

/// Asserts that a write was refused and that its problems name every one of
/// `sentences`.
fn refused_with(ok: bool, problems: &[String], sentences: &[String], what: &str) {
    assert!(!ok, "{what}: not refused");
    assert!(!sentences.is_empty(), "{what}: nothing to name");
    for sentence in sentences {
        assert!(
            problems.contains(sentence),
            "{what}: {sentence:?} is not among {problems:?}"
        );
    }
}

/// Every file under `dir` with its bytes, a link listed and never entered:
/// what a link's target held, to compare before and after.
fn contents(dir: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    tree(dir)
        .into_iter()
        .map(|relative| {
            let path = dir.join(&relative);
            let bytes =
                (!links::is_link(&path) && path.is_file()).then(|| fs::read(&path).unwrap());
            (relative, bytes)
        })
        .collect()
}

#[test]
fn refuses_every_name_and_path_that_reaches_outside_the_templates_folder_and_writes_nothing() {
    let world = World::new();
    write(&world.root.join("sibling").join("keep.txt"), "kept");
    write(&world.root.join("outside.md"), "outside");
    assert!(world.create("kit", None));
    let memory = write(&world.root.join("claude").join("CLAUDE.md"), "# mine\n");
    let before = contents(&world.root);

    for name in ["../escape", "..\\escape", "/escape"] {
        let sentences = template_id_problems(name);
        let created = create_template(
            world.conn(),
            &world.templates,
            &CreateTemplateRequest {
                name: name.into(),
                label: None,
                description: None,
            },
        );
        refused_with(created.ok, &created.problems, &sentences, "create");
        let renamed = rename_template(
            &world.templates,
            &RenameTemplateRequest {
                template: "kit".into(),
                name: name.into(),
            },
        );
        refused_with(renamed.ok, &renamed.problems, &sentences, "rename to");
        let saved = save(&world, &world.root, name, "", vec![]);
        refused_with(saved.ok, &saved.problems, &sentences, "save a folder as");
    }
    for template in ["../sibling", "..\\sibling"] {
        let sentences = template_id_problems(template);
        let deleted = delete_template(&world.templates, template);
        refused_with(deleted.ok, &deleted.problems, &sentences, "delete");
        let described = write_template_metadata(
            world.conn(),
            &world.templates,
            &TemplateMetadataRequest {
                template: template.into(),
                label: "Taken over".into(),
                description: String::new(),
            },
        );
        refused_with(described.ok, &described.problems, &sentences, "describe");
        let renamed = rename_template(
            &world.templates,
            &RenameTemplateRequest {
                template: template.into(),
                name: "stolen".into(),
            },
        );
        refused_with(renamed.ok, &renamed.problems, &sentences, "rename");
        let imported = import_into_template(
            &world.templates,
            template,
            &[ImportFile {
                source: memory.clone(),
                target: ".claude/CLAUDE.md".into(),
            }],
        );
        refused_with(imported.ok, &imported.problems, &sentences, "import into");
    }
    for path in ["../../outside.md", "..\\..\\outside.md"] {
        let filled = make_substitutable(
            &world.templates,
            &TemplateFileRequest {
                template: "kit".into(),
                path: path.into(),
            },
        );
        let sentence = format!("\"{path}\" is not a file in this template.");
        refused_with(filled.ok, &filled.problems, &[sentence], "fill");
    }

    assert_eq!(contents(&world.root), before);
    assert_eq!(tree(&world.templates.join("kit")), ["template.yaml"]);
}
