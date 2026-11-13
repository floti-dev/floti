//! The profile rows in a store of the test's own, and what a save, export and
//! import answer over them.

use std::path::{Path, PathBuf};

use floti_protocol::channels::common::PermissionMode;
use floti_protocol::channels::sessions::{
    EffortLevel, ProfileDocument, ProfileDraft, SaveProfileRequest,
};
use rusqlite::Connection;

use super::{export, import, profile_draft, profile_to_yaml, save, store};
use crate::store::Store;
use crate::testkit::TestDirs;

/// A store of its own in a test folder, and the home profiles are written
/// against.
struct World {
    store: Store,
    home: PathBuf,
    /// Last: fields drop in order, and the folder goes only once the store is closed.
    dirs: TestDirs,
}

impl World {
    fn new() -> Self {
        let dirs = TestDirs::new();
        Self {
            store: Store::open(&dirs.data).unwrap(),
            home: dirs.home.clone(),
            dirs,
        }
    }

    fn conn(&self) -> &Connection {
        self.store.connection()
    }

    fn home(&self) -> Option<&Path> {
        Some(self.home.as_path())
    }

    /// A draft rooted in the test folder, with one overlay and one access
    /// path under the root.
    fn draft(&self) -> ProfileDraft {
        let workspace = self.dirs.root().join("workspace");
        ProfileDraft {
            name: "Acme cloud sync".into(),
            root: text(&workspace),
            overlays: vec![text(&workspace.join("repos").join("acme"))],
            access: vec![text(&workspace.join("repos").join("other-repo"))],
            model: Some("opus".into()),
            effort: Some(EffortLevel::High),
            permission_mode: Some(PermissionMode::Auto),
            agent: None,
            mcp: vec!["clickup".into()],
            opening_prompt: Some("/recap".into()),
            pinned_order: None,
            session_agents: None,
            remote_control: None,
        }
    }

    fn named(&self, name: &str) -> ProfileDraft {
        ProfileDraft {
            name: name.into(),
            ..self.draft()
        }
    }

    fn names(&self) -> Vec<String> {
        store::list(self.conn())
            .unwrap()
            .into_iter()
            .map(|profile| profile.name)
            .collect()
    }
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

// store

#[test]
fn a_profile_is_stored_and_read_back_with_every_field() {
    let world = World::new();
    let created = store::create(world.conn(), &world.draft()).unwrap();
    assert!(created.id > 0);
    assert_eq!(profile_draft(&created), world.draft());
    let read = store::read(world.conn(), created.id).unwrap().unwrap();
    assert_eq!(profile_draft(&read), world.draft());
}

#[test]
fn a_profile_updates_in_place_and_stamps_updated_at() {
    let world = World::new();
    let created = store::create(world.conn(), &world.draft()).unwrap();
    let updated = store::update(
        world.conn(),
        created.id,
        &ProfileDraft {
            model: Some("sonnet".into()),
            mcp: Vec::new(),
            ..world.draft()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(updated.model.as_deref(), Some("sonnet"));
    assert_eq!(updated.mcp, Vec::<String>::new());
    assert!(updated.updated_at >= created.updated_at);
    assert_eq!(store::list(world.conn()).unwrap().len(), 1);
}

#[test]
fn a_profile_that_turns_the_agents_tools_off_keeps_the_choice_and_another_profile_does_not_inherit_it()
 {
    let world = World::new();
    let off = store::create(
        world.conn(),
        &ProfileDraft {
            session_agents: Some(false),
            ..world.draft()
        },
    )
    .unwrap();
    let other = store::create(world.conn(), &world.named("Other")).unwrap();
    assert_eq!(off.session_agents, Some(false));
    assert_eq!(
        store::read(world.conn(), off.id)
            .unwrap()
            .unwrap()
            .session_agents,
        Some(false)
    );
    assert_eq!(other.session_agents, None, "a profile follows the setting");
    let listed = store::list(world.conn()).unwrap();
    let choice = |id: u64| {
        listed
            .iter()
            .find(|profile| profile.id == id)
            .unwrap()
            .session_agents
    };
    assert_eq!(choice(off.id), Some(false));
    assert_eq!(choice(other.id), None);

    // Saving it again with the choice taken back follows the setting again.
    let back = store::update(world.conn(), off.id, &world.draft())
        .unwrap()
        .unwrap();
    assert_eq!(back.session_agents, None);
}

#[test]
fn a_profile_cannot_turn_the_agents_tools_on_against_the_setting_so_on_is_kept_as_following_it() {
    let world = World::new();
    let created = store::create(
        world.conn(),
        &ProfileDraft {
            session_agents: Some(true),
            ..world.draft()
        },
    )
    .unwrap();
    assert_eq!(created.session_agents, None);
}

#[test]
fn a_profile_that_turns_remote_control_on_keeps_the_choice_and_another_profile_does_not_inherit_it()
{
    let world = World::new();
    let on = store::create(
        world.conn(),
        &ProfileDraft {
            remote_control: Some(true),
            ..world.draft()
        },
    )
    .unwrap();
    let other = store::create(world.conn(), &world.named("Other")).unwrap();
    assert_eq!(on.remote_control, Some(true));
    assert_eq!(
        store::read(world.conn(), on.id)
            .unwrap()
            .unwrap()
            .remote_control,
        Some(true)
    );
    assert_eq!(other.remote_control, None, "a profile starts without it");

    // Saving it again with the choice taken back turns it off.
    let back = store::update(world.conn(), on.id, &world.draft())
        .unwrap()
        .unwrap();
    assert_eq!(back.remote_control, None);

    // Turning it on later is a save in place, and false is kept as off.
    let again = store::update(
        world.conn(),
        on.id,
        &ProfileDraft {
            remote_control: Some(true),
            ..world.draft()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(again.remote_control, Some(true));
    let off = store::create(
        world.conn(),
        &ProfileDraft {
            remote_control: Some(false),
            ..world.named("Explicit off")
        },
    )
    .unwrap();
    assert_eq!(off.remote_control, None);
}

#[test]
fn updating_a_profile_that_has_been_deleted_finds_none() {
    let world = World::new();
    assert_eq!(
        store::update(world.conn(), 999, &world.draft()).unwrap(),
        None
    );
}

#[test]
fn a_delete_says_whether_there_was_anything_to_delete() {
    let world = World::new();
    let created = store::create(world.conn(), &world.draft()).unwrap();
    assert!(store::delete(world.conn(), created.id).unwrap());
    assert!(!store::delete(world.conn(), created.id).unwrap());
    assert_eq!(store::read(world.conn(), created.id).unwrap(), None);
    assert!(store::list(world.conn()).unwrap().is_empty());
}

#[test]
fn the_store_rejects_a_duplicate_name_which_is_what_makes_import_ask_for_a_new_one() {
    let world = World::new();
    store::create(world.conn(), &world.draft()).unwrap();
    assert!(store::create(world.conn(), &world.draft()).is_err());
}

#[test]
fn a_profile_is_found_by_name_without_case_the_way_a_person_types_it() {
    let world = World::new();
    store::create(world.conn(), &world.draft()).unwrap();
    assert_eq!(
        store::find_by_name(world.conn(), "acme cloud sync")
            .unwrap()
            .map(|profile| profile.name),
        Some("Acme cloud sync".to_owned())
    );
    assert_eq!(store::find_by_name(world.conn(), "nothing").unwrap(), None);
}

#[test]
fn an_import_of_something_already_here_is_given_a_free_name() {
    let world = World::new();
    store::create(world.conn(), &world.draft()).unwrap();
    assert_eq!(
        store::unique_name(world.conn(), "Acme cloud sync").unwrap(),
        "Acme cloud sync (2)"
    );
    store::create(world.conn(), &world.named("Acme cloud sync (2)")).unwrap();
    assert_eq!(
        store::unique_name(world.conn(), "Acme cloud sync").unwrap(),
        "Acme cloud sync (3)"
    );
}

// ordering

#[test]
fn pinned_profiles_come_first_in_their_order_then_the_rest_by_name() {
    let mut world = World::new();
    let a = store::create(world.conn(), &world.named("Alpha")).unwrap();
    let b = store::create(world.conn(), &world.named("Beta")).unwrap();
    let c = store::create(world.conn(), &world.named("Gamma")).unwrap();

    store::set_pinned(world.store.connection_mut(), &[c.id, a.id]).unwrap();
    assert_eq!(world.names(), ["Gamma", "Alpha", "Beta"]);
    assert_eq!(
        store::read(world.conn(), b.id)
            .unwrap()
            .unwrap()
            .pinned_order,
        None
    );
}

#[test]
fn a_profile_left_out_of_the_pin_list_is_unpinned() {
    let mut world = World::new();
    let a = store::create(world.conn(), &world.named("Alpha")).unwrap();
    let b = store::create(world.conn(), &world.named("Beta")).unwrap();
    let c = store::create(world.conn(), &world.named("Gamma")).unwrap();
    store::set_pinned(world.store.connection_mut(), &[c.id, a.id]).unwrap();
    store::set_pinned(world.store.connection_mut(), &[b.id]).unwrap();

    assert_eq!(world.names(), ["Beta", "Alpha", "Gamma"]);
    for profile in store::list(world.conn()).unwrap() {
        assert_eq!(
            profile.pinned_order.is_some(),
            profile.id == b.id,
            "{} is pinned only if it was in the list",
            profile.name
        );
    }
}

// save

#[test]
fn a_saved_profile_is_listed_and_a_second_one_by_the_same_name_is_refused() {
    let world = World::new();
    let saved = save(
        world.conn(),
        &SaveProfileRequest {
            id: None,
            draft: world.named("  hub dev  "),
        },
    )
    .unwrap();
    assert_eq!(saved.problems, Vec::<String>::new());
    let listed = store::list(world.conn()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(profile_draft(&listed[0]), world.named("hub dev"));
    assert_eq!(Some(listed[0].id), saved.profile.map(|profile| profile.id));

    let clash = save(
        world.conn(),
        &SaveProfileRequest {
            id: None,
            draft: world.named("HUB DEV"),
        },
    )
    .unwrap();
    assert_eq!(clash.profile, None);
    assert_eq!(
        clash.problems,
        ["A profile named \u{201c}hub dev\u{201d} already exists."]
    );
    assert_eq!(store::list(world.conn()).unwrap().len(), 1);
}

#[test]
fn a_draft_that_fails_validation_is_refused_with_its_problems_and_nothing_is_written() {
    let world = World::new();
    let refused = save(
        world.conn(),
        &SaveProfileRequest {
            id: None,
            draft: world.named(" "),
        },
    )
    .unwrap();
    assert_eq!(refused.profile, None);
    assert_eq!(refused.problems, ["A profile needs a name."]);
    assert!(store::list(world.conn()).unwrap().is_empty());
}

#[test]
fn a_draft_with_several_problems_is_refused_with_every_one_of_them() {
    let world = World::new();
    let broken = ProfileDraft {
        root: " ".into(),
        ..world.named("  ")
    };
    let refused = save(
        world.conn(),
        &SaveProfileRequest {
            id: None,
            draft: broken,
        },
    )
    .unwrap();
    assert_eq!(refused.profile, None);
    assert_eq!(
        refused.problems,
        [
            "A profile needs a name.",
            "A profile needs a root directory."
        ]
    );
    assert!(store::list(world.conn()).unwrap().is_empty());
}

#[test]
fn saving_over_a_profile_that_has_gone_says_so() {
    let world = World::new();
    let refused = save(
        world.conn(),
        &SaveProfileRequest {
            id: Some(42),
            draft: world.draft(),
        },
    )
    .unwrap();
    assert_eq!(refused.profile, None);
    assert_eq!(refused.problems, ["That profile no longer exists."]);
    assert!(store::list(world.conn()).unwrap().is_empty());
}

#[test]
fn a_profile_keeps_its_own_name_when_it_is_saved_again() {
    let world = World::new();
    let created = store::create(world.conn(), &world.draft()).unwrap();
    let saved = save(
        world.conn(),
        &SaveProfileRequest {
            id: Some(created.id),
            draft: ProfileDraft {
                model: Some("haiku".into()),
                ..world.draft()
            },
        },
    )
    .unwrap();
    assert_eq!(saved.problems, Vec::<String>::new());
    assert_eq!(saved.profile.unwrap().model.as_deref(), Some("haiku"));
}

// export and import

#[test]
fn export_then_delete_then_import_leaves_the_profile_intact() {
    let world = World::new();
    let created = store::create(world.conn(), &world.draft()).unwrap();
    let document = export(world.conn(), created.id, world.home())
        .unwrap()
        .unwrap();
    assert_eq!(document.file_name, "acme-cloud-sync.floti-profile.yaml");
    assert_eq!(
        document.yaml,
        profile_to_yaml(&profile_draft(&created), world.home())
    );

    assert!(store::delete(world.conn(), created.id).unwrap());
    assert!(store::list(world.conn()).unwrap().is_empty());

    let imported = import(world.conn(), &document, world.home()).unwrap();
    let profile = imported.profile.unwrap();
    assert_eq!(profile_draft(&profile), world.draft());
    assert_ne!(profile.id, created.id);
    assert_eq!(imported.renamed_to, None);
}

#[test]
fn an_import_of_a_name_already_taken_is_renamed_rather_than_refused_or_overwritten() {
    let world = World::new();
    let created = store::create(world.conn(), &world.draft()).unwrap();
    let document = export(world.conn(), created.id, world.home())
        .unwrap()
        .unwrap();
    let imported = import(world.conn(), &document, world.home()).unwrap();
    let profile = imported.profile.unwrap();
    assert_eq!(imported.renamed_to.as_deref(), Some("Acme cloud sync (2)"));
    assert_eq!(profile.root, created.root);
    assert_eq!(profile.overlays, created.overlays);
    assert_eq!(store::list(world.conn()).unwrap().len(), 2);
}

#[test]
fn an_import_of_a_document_that_is_not_a_profile_names_the_file_and_writes_nothing() {
    let world = World::new();
    let rootless = import(
        world.conn(),
        &ProfileDocument {
            file_name: "broken.yaml".into(),
            yaml: "name: x\n".into(),
        },
        world.home(),
    )
    .unwrap();
    assert_eq!(rootless.profile, None);
    assert_eq!(
        rootless.error.as_deref(),
        Some("broken.yaml: A profile needs a `root`.")
    );
    let list = import(
        world.conn(),
        &ProfileDocument {
            file_name: "list.yaml".into(),
            yaml: "- a\n- b\n".into(),
        },
        world.home(),
    )
    .unwrap();
    assert_eq!(list.profile, None);
    assert_eq!(
        list.error.as_deref(),
        Some("list.yaml: A profile file must be a YAML mapping.")
    );
    assert!(store::list(world.conn()).unwrap().is_empty());
}

#[test]
fn exporting_a_profile_that_has_gone_says_so() {
    let world = World::new();
    assert_eq!(
        export(world.conn(), 7, None).unwrap(),
        Err("That profile no longer exists.".to_owned())
    );
}
