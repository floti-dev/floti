use std::fs;

use floti_protocol::channels::workspace::{CreateWorkspaceRequest, WorkspaceMode};

use super::{SHIPPED_TEMPLATES, seed_templates};
use crate::workspace::templates::reader::list_templates;
use crate::workspace::templates::testing::{World, ids, read, tree, write};
use crate::workspace::templates::workspace::create_workspace;

// ---------------------------------------------------------------------------
// Seeding the templates folder
// ---------------------------------------------------------------------------

fn shipped(relative: &str) -> &'static str {
    SHIPPED_TEMPLATES
        .iter()
        .find(|(path, _)| *path == relative)
        .map(|(_, content)| *content)
        .unwrap()
}

#[test]
fn writes_only_the_readme_into_a_folder_that_is_not_there() {
    let world = World::new();
    let dir = world.root.join("seeded");
    let result = seed_templates(&dir);

    assert!(result.seeded);
    assert_eq!(result.problem, None);
    assert_eq!(result.created, ["README.md"]);
    assert_eq!(tree(&dir), ["README.md"]);
    assert_eq!(read(&dir.join("README.md")), shipped("README.md"));

    // No template is a folder here: the picker is Starter alone.
    let listing = list_templates(&dir);
    assert_eq!(ids(&dir), ["starter"]);
    assert!(listing.problems.is_empty(), "{:?}", listing.problems);
}

#[test]
fn the_readme_says_how_to_make_a_template_and_points_at_no_example() {
    let readme = shipped("README.md");
    assert!(!readme.to_lowercase().contains("example"), "{readme}");
    assert!(readme.contains("## Making one"), "{readme}");
    assert!(readme.contains("Save as template"), "{readme}");
    assert!(readme.contains("Import folder"), "{readme}");
    assert!(readme.contains("## Starter"), "{readme}");
    assert!(!readme.contains("Minimal"), "{readme}");
}

#[test]
fn overwrites_nothing_on_a_second_call_however_the_files_have_been_edited() {
    let world = World::new();
    let dir = world.root.join("seeded");
    seed_templates(&dir);
    fs::write(dir.join("README.md"), "mine now").unwrap();

    let again = seed_templates(&dir);
    assert!(!again.seeded);
    assert!(again.created.is_empty());
    assert_eq!(read(&dir.join("README.md")), "mine now");
}

#[test]
fn writes_nothing_into_a_folder_that_is_there_even_empty_or_missing_the_readme() {
    let world = World::new();
    let empty = world.root.join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert!(!seed_templates(&empty).seeded);
    assert!(tree(&empty).is_empty());

    let trimmed = world.root.join("trimmed");
    seed_templates(&trimmed);
    fs::remove_file(trimmed.join("README.md")).unwrap();
    let again = seed_templates(&trimmed);
    assert!(!again.seeded);
    assert!(!trimmed.join("README.md").exists());
}

#[test]
fn re_seeds_once_the_folder_is_gone_which_is_the_whole_of_reset() {
    let world = World::new();
    let dir = world.root.join("seeded");
    seed_templates(&dir);
    fs::write(dir.join("README.md"), "mine now").unwrap();
    fs::remove_dir_all(&dir).unwrap();

    assert!(seed_templates(&dir).seeded);
    assert_eq!(read(&dir.join("README.md")), shipped("README.md"));
}

#[test]
fn leaves_an_example_an_older_floti_seeded_exactly_as_it_was_and_still_offers_it() {
    let world = World::new();
    let dir = world.root.join("seeded");
    let older = [
        ("README.md", "# Workspace templates\n\nThe old README.\n"),
        (
            "example/template.yaml",
            "label: \"Example\"\ndescription: \"A CLAUDE.md naming the workspace.\"\norder: 10\n",
        ),
        ("example/CLAUDE.md.tpl", "# {{NAME}}\n"),
        ("example/dot-claude/skills/.gitkeep", ""),
        ("example/repos/.gitkeep", ""),
    ];
    for (relative, content) in older {
        write(&dir.join(relative), content);
    }
    let before = tree(&dir);

    let again = seed_templates(&dir);

    assert!(!again.seeded);
    assert!(again.created.is_empty());
    assert_eq!(tree(&dir), before);
    for (relative, content) in older {
        assert_eq!(read(&dir.join(relative)), content, "{relative}");
    }
    // Starter reaches this user anyway, first, with their example after it.
    assert_eq!(ids(&dir), ["starter", "example"]);

    // And making a workspace from Starter writes nothing into the folder.
    let made = create_workspace(
        &CreateWorkspaceRequest {
            mode: WorkspaceMode::New,
            dir: world.root.display().to_string(),
            name: Some("work".into()),
            template: Some("starter".into()),
        },
        Some(&dir),
    )
    .unwrap();
    assert!(made.problems.is_empty(), "{:?}", made.problems);
    assert_eq!(tree(&dir), before);
}
