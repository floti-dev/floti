use std::fs;

use floti_protocol::channels::workspace::{CreateHarnessRequest, HarnessMode};

use super::{SHIPPED_TEMPLATES, seed_templates};
use crate::workspace::templates::harness::create_harness;
use crate::workspace::templates::reader::list_templates;
use crate::workspace::templates::testing::{World, ids, line_matches, read, tree};

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
fn writes_the_readme_and_the_example_into_a_folder_that_is_not_there() {
    let world = World::new();
    let dir = world.root.join("seeded");
    let result = seed_templates(&dir);

    assert!(result.seeded);
    assert_eq!(result.problem, None);
    let mut created = result.created.clone();
    created.sort();
    let mut expected: Vec<&str> = SHIPPED_TEMPLATES.iter().map(|(path, _)| *path).collect();
    expected.sort_unstable();
    assert_eq!(created, expected);
    assert!(tree(&dir).contains(&"README.md".to_owned()));

    // And what it wrote is a template the engine can read back.
    let listing = list_templates(&dir);
    assert_eq!(ids(&dir), ["minimal", "example"]);
    assert!(listing.problems.is_empty(), "{:?}", listing.problems);
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
fn produces_a_harness_from_the_shipped_example() {
    let world = World::new();
    let dir = world.root.join("seeded");
    seed_templates(&dir);

    let result = create_harness(
        &CreateHarnessRequest {
            mode: HarnessMode::New,
            dir: world.root.display().to_string(),
            name: Some("work".into()),
            template: Some("example".into()),
        },
        Some(&dir),
    )
    .unwrap();

    assert!(result.problems.is_empty(), "{:?}", result.problems);
    assert_eq!(
        tree(&world.root.join("work")),
        [
            ".claude",
            ".claude/skills",
            "CLAUDE.md",
            "harness.yaml",
            "repos"
        ]
    );
    assert!(line_matches(
        &read(&world.root.join("work").join("CLAUDE.md")),
        "# work"
    ));
}
