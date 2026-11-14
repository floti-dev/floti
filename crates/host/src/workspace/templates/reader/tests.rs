use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use floti_protocol::channels::workspace::HarnessMode;

use super::{
    TemplateValues, list_templates, preview_template, substitute_template, template_id_problems,
    template_path_value,
};
use crate::testkit::link_folder;
use crate::workspace::templates::testing::{World, ids, read, tree, write};

// ---------------------------------------------------------------------------
// Listing the templates folder
// ---------------------------------------------------------------------------

#[test]
fn puts_the_built_in_first_however_the_folder_sorts() {
    let world = World::new();
    world.plant("aaa", &[("template.yaml", "label: \"Aaa\"\n")]);

    let listing = list_templates(&world.templates);
    assert_eq!(world.listed_ids(), ["minimal", "aaa"]);
    assert!(listing.templates[0].built_in);
    assert_eq!(listing.dir, world.templates.display().to_string());
}

#[test]
fn is_a_one_row_picker_when_the_folder_is_not_there_at_all() {
    let world = World::new();
    let listing = list_templates(&world.root.join("nothing-here"));
    assert_eq!(ids(&world.root.join("nothing-here")), ["minimal"]);
    assert!(listing.problems.is_empty());
}

#[test]
fn reads_label_description_and_order_and_falls_back_to_the_folder_name() {
    let world = World::new();
    world.plant(
        "described",
        &[(
            "template.yaml",
            "label: \"A label\"\ndescription: \"A sentence.\"\norder: 3\n",
        )],
    );
    world.plant("bare", &[("a.txt", "x")]);

    let listing = list_templates(&world.templates);
    let described = &listing.templates[1];
    assert_eq!(
        (
            described.id.as_str(),
            described.label.as_str(),
            described.description.as_deref(),
            described.order,
            described.built_in
        ),
        ("described", "A label", Some("A sentence."), Some(3), false)
    );
    let bare = &listing.templates[2];
    assert_eq!(
        (
            bare.id.as_str(),
            bare.label.as_str(),
            bare.description.as_deref(),
            bare.order,
            bare.built_in
        ),
        ("bare", "bare", None, None, false)
    );
    assert_eq!(listing.templates.len(), 3);
}

#[test]
fn sorts_by_order_then_label_with_the_unordered_ones_last() {
    let world = World::new();
    world.plant("one", &[("template.yaml", "label: \"zeta\"\norder: 1\n")]);
    world.plant("two", &[("template.yaml", "label: \"alpha\"\norder: 2\n")]);
    world.plant("three", &[("template.yaml", "label: \"beta\"\n")]);
    world.plant("four", &[("template.yaml", "label: \"aardvark\"\n")]);

    let labels: Vec<String> = list_templates(&world.templates)
        .templates
        .into_iter()
        .map(|choice| choice.label)
        .collect();
    assert_eq!(labels, ["Minimal", "zeta", "alpha", "aardvark", "beta"]);
}

#[test]
fn skips_one_malformed_template_with_a_sentence_and_keeps_the_rest() {
    let world = World::new();
    world.plant("good", &[("template.yaml", "label: \"Good\"\n")]);
    world.plant(
        "broken",
        &[("template.yaml", "label: \"unterminated\n  - [\n")],
    );

    let listing = list_templates(&world.templates);
    assert_eq!(world.listed_ids(), ["minimal", "good"]);
    assert_eq!(listing.problems.len(), 1);
    assert!(
        listing.problems[0].contains("broken"),
        "{:?}",
        listing.problems
    );
}

#[test]
fn will_not_let_a_folder_take_the_built_in_name() {
    let world = World::new();
    world.plant("minimal", &[("template.yaml", "label: \"Impostor\"\n")]);

    let listing = list_templates(&world.templates);
    assert_eq!(world.listed_ids(), ["minimal"]);
    assert_eq!(listing.templates[0].label, "Minimal");
    assert!(
        listing.problems[0].contains("built-in"),
        "{:?}",
        listing.problems
    );
}

#[test]
fn does_not_offer_a_link_as_a_template() {
    let world = World::new();
    let real = world.plant("real", &[("template.yaml", "label: \"Real\"\n")]);
    link_folder(&real, &world.templates.join("linked"));

    let listing = list_templates(&world.templates);
    assert_eq!(world.listed_ids(), ["minimal", "real"]);
    assert!(
        listing.problems.join(" ").contains("linked"),
        "{:?}",
        listing.problems
    );
}

// ---------------------------------------------------------------------------
// previewTemplate
// ---------------------------------------------------------------------------

#[test]
fn describes_the_minimal_scaffold_per_mode() {
    let world = World::new();
    let for_new = preview_template(&world.templates, "minimal", Some(HarnessMode::New));
    assert_eq!(for_new.entries, ["harness.yaml", "repos/", ".claude/"]);

    let for_convert = preview_template(&world.templates, "anything", Some(HarnessMode::Convert));
    assert_eq!(for_convert.entries, ["harness.yaml", ".claude/"]);
    assert_eq!(for_convert.template, "minimal");
}

#[test]
fn lists_a_template_as_the_writer_would_write_it_manifest_first() {
    let world = World::new();
    world.plant(
        "demo",
        &[
            ("template.yaml", "label: \"Demo\"\n"),
            ("CLAUDE.md.tpl", "# {{NAME}}\n"),
            ("dot-claude/settings.json", "{}"),
            ("notes/.gitkeep", ""),
        ],
    );

    let preview = preview_template(&world.templates, "demo", None);
    assert_eq!(
        preview.entries,
        [
            "harness.yaml",
            ".claude/settings.json",
            "CLAUDE.md",
            "notes/"
        ]
    );
    assert!(preview.note.contains("Demo"), "{}", preview.note);
    assert!(preview.problems.is_empty(), "{:?}", preview.problems);
}

#[test]
fn names_exactly_what_the_writer_then_writes_for_the_built_in_and_for_a_template() {
    let world = World::new();
    world.plant(
        "demo",
        &[
            ("template.yaml", "label: \"Demo\"\n"),
            ("CLAUDE.md.tpl", "# {{NAME}}\n"),
            ("dot-claude/settings.json", "{}"),
            ("notes/.gitkeep", ""),
            ("tools/run.mjs", "export {}\n"),
        ],
    );

    for template in ["minimal", "demo"] {
        let preview = preview_template(&world.templates, template, Some(HarnessMode::New));
        let name = format!("from-{template}");
        let result = world.harness(&name, Some(template));
        assert!(
            result.problems.is_empty(),
            "{template}: {:?}",
            result.problems
        );

        // Files, and folders with nothing in them: what a list of what was written names.
        let written = tree(&world.root.join(&name));
        let leaves: Vec<String> = written
            .iter()
            .filter(|path| {
                !written
                    .iter()
                    .any(|other| other.starts_with(&format!("{path}/")))
            })
            .cloned()
            .collect();
        assert_eq!(preview.entries[0], "harness.yaml", "{template}");
        let mut previewed: Vec<String> = preview
            .entries
            .iter()
            .map(|entry| entry.trim_end_matches('/').to_owned())
            .collect();
        previewed.sort();
        assert_eq!(previewed, leaves, "{template}");
    }
}

#[test]
fn says_so_rather_than_listing_nothing_for_a_template_that_is_not_there() {
    let world = World::new();
    let preview = preview_template(&world.templates, "absent", None);
    assert!(preview.entries.is_empty());
    assert_eq!(
        preview.problems,
        ["There is no template called \"absent\"."]
    );
}

// ---------------------------------------------------------------------------
// substituteTemplate, templatePathValue, templateIdProblems
// ---------------------------------------------------------------------------

fn values() -> TemplateValues {
    TemplateValues {
        name: "work".into(),
        created_at: "2026-08-15T00:00:00.000Z".into(),
        template: "demo".into(),
        path: "C:/Users/me/.harness/work".into(),
    }
}

#[test]
fn fills_in_the_four_placeholders_every_occurrence() {
    assert_eq!(
        substitute_template(
            "{{NAME}} {{NAME}} {{TEMPLATE}} {{CREATED_AT}} {{PATH}} {{PATH}}",
            &values()
        ),
        "work work demo 2026-08-15T00:00:00.000Z C:/Users/me/.harness/work C:/Users/me/.harness/work"
    );
}

#[test]
fn leaves_anything_else_exactly_as_written() {
    let source = "${{ github.sha }} {{ NAME }} {{name}} {{UNKNOWN}}";
    assert_eq!(substitute_template(source, &values()), source);
}

#[test]
fn spells_the_platform_path_with_forward_slashes_spaces_intact() {
    let path: PathBuf = ["C:", "Users", "a b", ".harness", "work"]
        .join(MAIN_SEPARATOR.encode_utf8(&mut [0; 4]))
        .into();
    assert_eq!(template_path_value(&path), "C:/Users/a b/.harness/work");
}

#[test]
fn leaves_a_backslash_alone_where_it_is_a_filename_character() {
    if MAIN_SEPARATOR != '/' {
        return;
    }
    assert_eq!(
        template_path_value(Path::new("/home/me/odd\\name")),
        "/home/me/odd\\name"
    );
}

#[test]
fn refuses_a_path_where_a_name_belongs() {
    for id in [
        "..",
        "../x",
        "..\\x",
        "a/b",
        "a\\b",
        "C:\\Windows",
        "/etc",
        "",
    ] {
        assert!(!template_id_problems(id).is_empty(), "{id}");
    }
}

#[test]
fn passes_an_ordinary_folder_name() {
    for id in ["demo", "my-template", "Work Space", "a1"] {
        assert!(template_id_problems(id).is_empty(), "{id}");
    }
}

#[test]
fn names_a_link_in_a_template_and_previews_nothing_behind_it() {
    let world = World::new();
    let canary = world.root.join("canary");
    write(&canary.join("do-not-touch.txt"), "untouched");
    world.plant(
        "hostile",
        &[
            ("template.yaml", "label: \"Hostile\"\n"),
            ("ok.txt", "fine"),
        ],
    );
    link_folder(&canary, &world.templates.join("hostile").join("escape"));

    let preview = preview_template(&world.templates, "hostile", Some(HarnessMode::New));

    assert_eq!(preview.entries, ["harness.yaml", "ok.txt"]);
    assert!(
        preview.problems.join(" ").contains("escape"),
        "{:?}",
        preview.problems
    );
    assert_eq!(tree(&canary), ["do-not-touch.txt"]);
    assert_eq!(read(&canary.join("do-not-touch.txt")), "untouched");
}
