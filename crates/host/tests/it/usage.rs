//! Usage through a real host on its socket: the latest raw reading a client
//! asks for, each new one pushed on `usage:changed` as the agent rewrites its
//! figures, a reading the host could not take, and a host with no backend that
//! declares usage. Reading and shaping the cached limits are unit tests of
//! `agent::claude_code::usage`, and the host's readings of `usage`.
//!
//! Every folder is temporary: a home with the agent's config folder in it,
//! and the host's data folder. No agent runs; `.claude.json` is written as
//! Claude Code writes it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use floti_host::agent::AgentBackend;
use floti_host::agent::claude_code::{ClaudeCodeBackend, ClaudeCodeSettings, usage_view};
use floti_host::platform::process_tree::Native as NativeTree;
use floti_host::server::{HostConfig, shell_backend};
use floti_host::testkit::{PATIENCE, RunningHost, TestClient, TestDirs};
use floti_host::usage::same_readings;
use floti_protocol::channel::from_payload;
use floti_protocol::channels::host::HostStopReason;
use floti_protocol::channels::sessions::{
    ReadUsage, UsageChanged, UsageGroup, UsageProblemKind, UsageSnapshot,
};
use floti_protocol::{BackendId, EventChannel, Message};
use serde_json::{Value, json};

/// 2026-08-10T09:00:00Z, core's `NOW`.
const NOW: u64 = 1_786_352_400_000;

/// A home with Claude Code's config folder in it.
struct Claude {
    dirs: TestDirs,
    config: PathBuf,
}

impl Claude {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let config = dirs.home.join(".claude");
        std::fs::create_dir_all(config.join("projects")).unwrap();
        Self { dirs, config }
    }

    fn backend(&self) -> ClaudeCodeBackend {
        ClaudeCodeBackend::new(
            ClaudeCodeSettings {
                search_first: Vec::new(),
                program: "claude".into(),
                leading_args: Vec::new(),
                config_dir: self.config.clone(),
                pass_config_dir: true,
                api_key_set: false,
            },
            Arc::new(NativeTree),
        )
    }

    /// Where the CLI keeps `.claude.json` for the default config folder:
    /// beside it.
    fn beside(&self) -> PathBuf {
        self.dirs.home.join(".claude.json")
    }

    fn write(&self, root: &Value) {
        std::fs::write(self.beside(), root.to_string()).unwrap();
    }

    fn config(&self) -> HostConfig {
        HostConfig {
            run: Default::default(),
            folders: self.dirs.folders(),
            endpoint: self.dirs.endpoint(),
            backends: vec![Arc::new(self.backend()) as Arc<dyn AgentBackend>],
            inherited_env: Vec::new(),
        }
    }
}

/// The three limits a 2.1.225 reading carries.
fn limits() -> Value {
    json!([
        { "kind": "session", "group": "session", "percent": 51, "severity": "normal",
          "resets_at": "2026-08-10T13:10:00.721285+00:00", "scope": null, "is_active": true },
        { "kind": "weekly_all", "group": "weekly", "percent": 38, "severity": "warning",
          "resets_at": "2026-08-11T19:00:00.721310+00:00", "scope": null, "is_active": false },
        { "kind": "weekly_scoped", "group": "weekly", "percent": 21, "severity": "normal",
          "resets_at": "2026-08-11T18:59:59.721602+00:00",
          "scope": { "model": { "id": null, "display_name": "Fable" }, "surface": null },
          "is_active": false },
    ])
}

/// The 2.1.225 shape of `.claude.json`, with the fields that vary made
/// arguments.
fn claude_json(fetched_at_ms: Value, limits: Value) -> Value {
    json!({
        "numStartups": 42,
        "projects": {},
        "cachedUsageUtilization": {
            "fetchedAtMs": fetched_at_ms,
            "accountUuid": "a5438327-5c7d-4ecd-97cd-47307ff36d81",
            "utilization": {
                "five_hour": { "utilization": 51, "resets_at": "2026-08-10T13:10:00.721285+00:00" },
                "seven_day": { "utilization": 38, "resets_at": "2026-08-11T19:00:00.721310+00:00" },
                "tangelo": null,
                "nimbus_quill": { "utilization": 0, "resets_at": null },
                "limits": limits,
                "spend": { "used": { "amount_minor": 0, "currency": "USD" }, "enabled": false, "percent": 0 },
                "extra_usage": { "is_enabled": false, "daily": null, "weekly": null },
            },
        },
    })
}

fn problem(snapshot: &UsageSnapshot) -> Option<UsageProblemKind> {
    snapshot.problem.as_ref().map(|problem| problem.kind)
}

fn snapshot_state(message: Message) -> Vec<UsageSnapshot> {
    match message {
        Message::Snapshot(snapshot) => from_payload(&snapshot.state).unwrap(),
        other => panic!("expected a snapshot, got {other:?}"),
    }
}

/// The next `usage:changed` event on `subscription`.
async fn next_readings(client: &mut TestClient, subscription: u64) -> Vec<UsageSnapshot> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match client.next(left).await {
            Some(Message::Event(event)) if event.subscription == subscription => {
                return from_payload(&event.payload).unwrap();
            }
            Some(_) => {}
            None => panic!("no usage reading within {PATIENCE:?}"),
        }
    }
}

fn file_in(path: &Path) -> String {
    path.display().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_gets_the_latest_raw_reading_and_each_new_one_as_it_lands() {
    let world = Claude::new();
    let fetched = 1_786_000_000_000_u64;
    world.write(&claude_json(json!(fetched), limits()));
    let config = world.config();
    let endpoint = config.endpoint.clone();
    let host = RunningHost::start(config);
    let mut client = TestClient::connect(&endpoint, &[]).await;

    let read = client.request::<ReadUsage>(&()).await.unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].backend, BackendId::claude_code());
    assert_eq!(read[0].file, file_in(&world.beside()));
    assert_eq!(read[0].fetched_at_ms, Some(fetched));
    // Raw: every limit as the agent cached it, reset or not, for the client
    // to judge.
    assert_eq!(read[0].limits.len(), 3);
    assert_eq!(read[0].limits[0].group, UsageGroup::Session);
    assert_eq!(read[0].problem, None);

    let watched = snapshot_state(client.subscribe_on(4, UsageChanged::NAME).await);
    // How long the estimate took to sum differs on every reading.
    assert!(same_readings(&watched, &read), "{watched:?} != {read:?}");

    // The agent rewrites its cached figures: the reading arrives unasked.
    world.write(&claude_json(
        json!(fetched + 60_000),
        json!([{ "kind": "session", "group": "session", "percent": 64, "resets_at": null }]),
    ));
    let next = next_readings(&mut client, 4).await;
    assert_eq!(next[0].fetched_at_ms, Some(fetched + 60_000));
    assert_eq!(next[0].limits.len(), 1);
    assert_eq!(next[0].limits[0].percent, 64.0);
    let again = client.request::<ReadUsage>(&()).await.unwrap();
    assert!(same_readings(&again, &next), "{again:?} != {next:?}");
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reading_the_host_could_not_take_is_unknown_and_never_a_number() {
    let world = Claude::new();
    let config = world.config();
    let endpoint = config.endpoint.clone();
    let host = RunningHost::start(config);
    let mut client = TestClient::connect(&endpoint, &[]).await;

    let read = client.request::<ReadUsage>(&()).await.unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(problem(&read[0]), Some(UsageProblemKind::NoFile));
    assert!(read[0].limits.is_empty());
    assert_eq!(read[0].fetched_at_ms, None);
    assert!(usage_view(&read[0], NOW).buckets.is_empty());

    // Half a file is no better.
    let watched = snapshot_state(client.subscribe_on(5, UsageChanged::NAME).await);
    // How long the estimate took to sum differs on every reading.
    assert!(same_readings(&watched, &read), "{watched:?} != {read:?}");
    std::fs::write(world.beside(), "{\"cachedUsageUtilization\": {\"fetched").unwrap();
    let next = next_readings(&mut client, 5).await;
    assert_eq!(problem(&next[0]), Some(UsageProblemKind::NotJson));
    assert!(next[0].limits.is_empty());
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_whose_backends_declare_no_usage_answers_with_no_readings() {
    let dirs = TestDirs::new();
    let endpoint = dirs.endpoint();
    let host = RunningHost::start(HostConfig {
        run: Default::default(),
        folders: dirs.folders(),
        endpoint: endpoint.clone(),
        backends: vec![Arc::new(shell_backend(&dirs.user_env())) as Arc<dyn AgentBackend>],
        inherited_env: Vec::new(),
    });
    let mut client = TestClient::connect(&endpoint, &[]).await;
    assert!(client.request::<ReadUsage>(&()).await.unwrap().is_empty());
    let watched = snapshot_state(client.subscribe_on(6, UsageChanged::NAME).await);
    assert!(watched.is_empty());
    host.stop(HostStopReason::Stop).await;
}
