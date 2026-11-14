//! Saved profiles as a client sees them: the profile channels and
//! `profiles:changed` through a client connection to a running host. The
//! document, the rows and what a save, export and import answer are unit
//! tests of `workspace::profiles`, and the router's in
//! `server/features/profiles`. The launch is `profile:launch`'s, in
//! `tests/it/sessions.rs`.

use std::ffi::OsString;
use std::path::Path;
use std::time::Instant;

use floti_host::platform::socket::Endpoint;
use floti_host::server::{HostConfig, HostRun, backends};
use floti_host::testkit::{PATIENCE, RunningHost, TestClient, TestDirs};
use floti_host::workspace::profiles::{profile_draft, profile_to_yaml};
use floti_protocol::Message;
use floti_protocol::channel::{EventChannel, from_payload};
use floti_protocol::channels::common::PermissionMode;
use floti_protocol::channels::host::HostStopReason;
use floti_protocol::channels::sessions::{
    EffortLevel, ImportProfileResult, PinProfilesRequest, Profile, ProfileDelete, ProfileDeleted,
    ProfileDocument, ProfileDraft, ProfileExport, ProfileImport, ProfileList, ProfilePin,
    ProfileRef, ProfileSave, ProfilesChanged, SaveProfileRequest, SaveProfileResult,
};
use floti_protocol::channels::workspace::AppMode;

const PROFILES_CHANGED: u64 = 1;

fn text(path: &Path) -> String {
    path.display().to_string()
}

/// A host over a test's folders, with a client connected to it.
struct HostWorld {
    host: Option<RunningHost>,
    client: TestClient,
    endpoint: Endpoint,
    /// Last: fields drop in order, and the folder goes only once the host
    /// has let go of its store.
    dirs: TestDirs,
}

impl HostWorld {
    async fn start() -> Self {
        let dirs = TestDirs::new();
        let endpoint = dirs.endpoint();
        let host = RunningHost::start(HostConfig {
            folders: dirs.folders(),
            endpoint: endpoint.clone(),
            backends: backends(&dirs.user_env(), None),
            inherited_env: std::env::vars_os()
                .filter(|(key, _)| key != "SHELL")
                .chain([(OsString::from("SHELL"), OsString::from("/bin/sh"))])
                .collect(),
            run: HostRun {
                mode: AppMode::Dev,
                home: Some(dirs.home.clone()),
                node: None,
                desktop: false,
                keeper: None,
                wsl: None,
                config_read_only: false,
                updates: None,
            },
        });
        let client = TestClient::connect(&endpoint, &[]).await;
        Self {
            host: Some(host),
            client,
            endpoint,
            dirs,
        }
    }

    async fn stop(mut self) {
        if let Some(host) = self.host.take() {
            host.stop(HostStopReason::Stop).await;
        }
    }

    /// The router tests' draft: rooted at `hub` in the test folder, with
    /// one overlay inside it.
    fn draft(&self, name: &str) -> ProfileDraft {
        let root = self.dirs.root().join("hub");
        ProfileDraft {
            name: name.into(),
            root: text(&root),
            overlays: vec![text(&root.join("repos").join("tools"))],
            access: Vec::new(),
            model: Some("sonnet".into()),
            effort: Some(EffortLevel::Medium),
            permission_mode: Some(PermissionMode::Plan),
            agent: None,
            mcp: Vec::new(),
            opening_prompt: Some("/recap".into()),
            pinned_order: None,
            session_agents: None,
            remote_control: None,
        }
    }

    async fn save(&mut self, name: &str) -> SaveProfileResult {
        let draft = self.draft(name);
        self.client
            .request::<ProfileSave>(&SaveProfileRequest { id: None, draft })
            .await
            .unwrap()
    }

    /// Another client, subscribed to `profiles:changed`, and the names of
    /// the list it started with.
    async fn watcher(&self) -> (TestClient, Vec<String>) {
        let mut client = TestClient::connect(&self.endpoint, &[]).await;
        match client
            .subscribe_on(PROFILES_CHANGED, ProfilesChanged::NAME)
            .await
        {
            Message::Snapshot(snapshot) => {
                let listed: Vec<Profile> = from_payload(&snapshot.state).unwrap();
                (client, names_of(&listed))
            }
            other => panic!("profiles:changed starts with its state, not {other:?}"),
        }
    }
}

fn names_of(listed: &[Profile]) -> Vec<String> {
    listed.iter().map(|profile| profile.name.clone()).collect()
}

/// The names in the next list published on `profiles:changed`.
async fn next_list(client: &mut TestClient) -> Vec<String> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match client.next(left).await {
            Some(Message::Event(event)) if event.subscription == PROFILES_CHANGED => {
                let listed: Vec<Profile> = from_payload(&event.payload).unwrap();
                return names_of(&listed);
            }
            Some(_) => {}
            None => panic!("no profiles:changed event within {PATIENCE:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn over_a_client_connection_every_subscriber_to_profile_changes_gets_the_whole_list_after_each_change()
 {
    let mut world = HostWorld::start().await;
    let (mut first, first_start) = world.watcher().await;
    let (mut second, second_start) = world.watcher().await;
    assert_eq!(first_start, Vec::<String>::new());
    assert_eq!(second_start, first_start);

    let alpha = world.save("Alpha").await.profile.unwrap();
    let beta = world.save("Beta").await.profile.unwrap();
    let pinned = world
        .client
        .request::<ProfilePin>(&PinProfilesRequest {
            ids: vec![beta.id, alpha.id],
        })
        .await
        .unwrap();
    assert_eq!(
        names_of(&pinned),
        ["Beta", "Alpha"],
        "pins keep the order given"
    );
    let deleted: ProfileDeleted = world
        .client
        .request::<ProfileDelete>(&ProfileRef { id: alpha.id })
        .await
        .unwrap();
    assert!(deleted.deleted);

    let expected = [
        vec!["Alpha".to_owned()],
        vec!["Alpha".to_owned(), "Beta".to_owned()],
        vec!["Beta".to_owned(), "Alpha".to_owned()],
        vec!["Beta".to_owned()],
    ];
    for watcher in [&mut first, &mut second] {
        for list in &expected {
            assert_eq!(&next_list(watcher).await, list);
        }
    }
    let (_later, later_start) = world.watcher().await;
    assert_eq!(later_start, ["Beta"]);
    assert_eq!(
        names_of(&world.client.request::<ProfileList>(&()).await.unwrap()),
        ["Beta"]
    );

    world.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn over_a_client_connection_an_export_is_the_profiles_yaml_document_and_an_import_is_validated_before_it_is_saved()
 {
    let mut world = HostWorld::start().await;
    let saved = world.save("Acme cloud sync").await.profile.unwrap();
    let document: ProfileDocument = world
        .client
        .request::<ProfileExport>(&ProfileRef { id: saved.id })
        .await
        .unwrap();
    assert_eq!(document.file_name, "acme-cloud-sync.floti-profile.yaml");
    for line in [
        "name: Acme cloud sync",
        "- repos/tools",
        "permission_mode: plan",
    ] {
        assert!(document.yaml.contains(line), "{}", document.yaml);
    }
    assert_eq!(
        document.yaml,
        profile_to_yaml(&profile_draft(&saved), Some(&world.dirs.home)),
        "the export is the document the profile writes, against the host's home"
    );

    let (mut watcher, _) = world.watcher().await;
    let rootless: ImportProfileResult = world
        .client
        .request::<ProfileImport>(&ProfileDocument {
            file_name: "broken.yaml".into(),
            yaml: "name: x\n".into(),
        })
        .await
        .unwrap();
    assert_eq!(rootless.profile, None);
    assert_eq!(
        rootless.error.as_deref(),
        Some("broken.yaml: A profile needs a `root`.")
    );
    assert_eq!(
        names_of(&world.client.request::<ProfileList>(&()).await.unwrap()),
        ["Acme cloud sync"]
    );

    let imported: ImportProfileResult = world
        .client
        .request::<ProfileImport>(&document)
        .await
        .unwrap();
    let profile = imported.profile.unwrap();
    assert_eq!(imported.renamed_to.as_deref(), Some("Acme cloud sync (2)"));
    assert_eq!(profile.overlays, saved.overlays);
    assert_eq!(profile.root, saved.root);
    // The refused import told nobody: the first change heard is this one.
    assert_eq!(
        next_list(&mut watcher).await,
        ["Acme cloud sync", "Acme cloud sync (2)"]
    );

    let gone = world
        .client
        .request::<ProfileExport>(&ProfileRef { id: 404 })
        .await
        .unwrap_err();
    assert_eq!(gone.message, "That profile no longer exists.");

    world.stop().await;
}
