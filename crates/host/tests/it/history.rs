//! History and the transcript archive through a real host on its socket:
//! the start-up pass, a conversation that ends in the host, the index and
//! every subscriber to it, the archive's ceiling, and a host with no backend
//! that keeps history. The index, the archive and a pass over Claude Code's
//! history on their own are unit tests in `history`.
//!
//! Every folder is temporary: the data folder with `floti.db`, the home and
//! the agent's config folder in it. No agent runs; the transcripts are
//! written as Claude Code writes them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use floti_host::agent::AgentBackend;
use floti_host::agent::claude_code::{ClaudeCodeBackend, ClaudeCodeSettings};
use floti_host::history::archive;
use floti_host::platform::process_tree::Native as NativeTree;
use floti_host::server::HostConfig;
use floti_host::testkit::{PATIENCE, RunningHost, TestClient, TestDirs};
use floti_protocol::channel::from_payload;
use floti_protocol::channels::host::HostStopReason;
use floti_protocol::channels::sessions::{
    ArchiveChanged, ArchiveConversation, ArchiveSessionState, ArchiveStats, HistoryChanged,
    HistoryPage, HistoryProjects, HistoryPrompts, HistoryQuery, HistoryRefresh, HistoryRename,
    HistorySessionRef, HistorySessions, HistorySummary, ReadArchiveStats, ReadHistorySummary,
    RenameHistorySessionRequest, SessionStart, StartSessionRequest,
};
use floti_protocol::{BackendId, EventChannel, Message, Payload};
use serde_json::json;

fn ids(page: &HistoryPage) -> Vec<&str> {
    page.sessions
        .iter()
        .map(|session| session.session_id.as_str())
        .collect()
}

/// A home with Claude Code's config folder in it, and a data folder.
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

    fn project(&self, name: &str) -> PathBuf {
        self.dirs.folder(name)
    }

    /// Writes a transcript under `projects/<folder>/`, rows as the CLI writes
    /// them, all from `cwd`, and appends each user row to `history.jsonl`,
    /// as the CLI records every prompt submitted.
    fn transcript(
        &self,
        folder: &str,
        id: &str,
        cwd: &Path,
        rows: &[(&str, &str, u64, &str)],
    ) -> PathBuf {
        let dir = self.config.join("projects").join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{id}.jsonl"));
        let text: String = rows
            .iter()
            .map(|(kind, uuid, at, text)| transcript_row(kind, uuid, cwd, *at, text))
            .collect();
        std::fs::write(&file, text).unwrap();
        for (_, _, at, text) in rows.iter().filter(|(kind, ..)| *kind == "user") {
            self.prompt(id, cwd, *at, text);
        }
        file
    }

    /// The prompt record.
    fn history_file(&self) -> PathBuf {
        self.config.join("history.jsonl")
    }

    /// Appends one prompt to the prompt record, as the CLI writes it.
    fn prompt(&self, id: &str, cwd: &Path, at: u64, text: &str) {
        self.append_history(&history_line(id, cwd, at, text));
    }

    fn append_history(&self, text: &str) {
        use std::io::Write as _;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.history_file())
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
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

    /// A host whose Claude Code runs the fake `claude`
    /// (`test/fake-agents/claude-code.mjs`) under Node, keeping its records
    /// in this world's config folder as the CLI does.
    fn hosting_config(&self) -> HostConfig {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("test")
            .join("fake-agents")
            .join("claude-code.mjs");
        let backend = ClaudeCodeBackend::new(
            ClaudeCodeSettings {
                search_first: Vec::new(),
                program: floti_host::testkit::node().into(),
                leading_args: vec![script.into_os_string()],
                config_dir: self.config.clone(),
                pass_config_dir: true,
                api_key_set: false,
            },
            Arc::new(NativeTree),
        );
        HostConfig {
            inherited_env: std::env::vars_os().collect(),
            backends: vec![Arc::new(backend) as Arc<dyn AgentBackend>],
            ..self.config()
        }
    }
}

const ONE: &str = "11111111-1111-4111-8111-111111111111";
const TWO: &str = "22222222-2222-4222-8222-222222222222";

fn uuid(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

/// One transcript row as the CLI writes it. A `thinking` row is the
/// agent's row for one thinking block, with its signature.
fn transcript_row(kind: &str, uuid: &str, cwd: &Path, at_ms: u64, text: &str) -> String {
    let (kind, content) = match kind {
        "assistant" => (kind, json!([{ "type": "text", "text": text }])),
        "thinking" => (
            "assistant",
            json!([{ "type": "thinking", "thinking": text, "signature": "EqoBCkgIBxABGAIiQ" }]),
        ),
        _ => (kind, json!(text)),
    };
    let row = json!({
        "type": kind,
        "uuid": uuid,
        "sessionId": "ignored-for-the-file-name",
        "cwd": cwd.display().to_string(),
        "timestamp": iso(at_ms),
        "message": { "role": kind, "content": content },
    });
    format!("{row}\n")
}

fn history_line(id: &str, cwd: &Path, at_ms: u64, text: &str) -> String {
    let row = json!({
        "display": text,
        "pastedContents": {},
        "timestamp": at_ms,
        "project": cwd.display().to_string(),
        "sessionId": id,
    });
    format!("{row}\n")
}

fn iso(ms: u64) -> String {
    let (secs, millis) = (ms / 1000, ms % 1000);
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

const T0: u64 = 1_760_000_000_000;

/// `len` characters deflate cannot shorten much, different for each `seed`,
/// so a conversation's stored size stays near its raw size.
fn incompressible(seed: u64, len: usize) -> String {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            char::from(b'!' + ((state >> 33) % 94) as u8)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Through the host
// ---------------------------------------------------------------------------

/// The next event on `subscription`, within the test's patience.
async fn event_on(client: &mut TestClient, subscription: u64) -> Payload {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match client.next(left).await {
            Some(Message::Event(event)) if event.subscription == subscription => {
                return event.payload;
            }
            Some(_) => {}
            None => panic!("no event on subscription {subscription} within {PATIENCE:?}"),
        }
    }
}

/// Waits for an event on `subscription` that `ready` accepts.
async fn event_until<T: serde::de::DeserializeOwned>(
    client: &mut TestClient,
    subscription: u64,
    ready: impl Fn(&T) -> bool,
) -> T {
    loop {
        let state: T = from_payload(&event_on(client, subscription).await).unwrap();
        if ready(&state) {
            return state;
        }
    }
}

fn snapshot<T: serde::de::DeserializeOwned>(message: Message) -> T {
    match message {
        Message::Snapshot(snapshot) => from_payload(&snapshot.state).unwrap(),
        other => panic!("expected a snapshot, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_start_up_pass_indexes_and_archives_what_happened_while_the_host_was_closed() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    world.transcript(
        "a",
        ONE,
        &alpha,
        &[
            ("user", &uuid(1), T0, "fix the launcher grid"),
            ("assistant", &uuid(2), T0 + 1, "fixed it"),
        ],
    );
    let config = world.config();
    let endpoint = config.endpoint.clone();
    let host = RunningHost::start(config);
    let mut client = TestClient::connect(&endpoint, &[]).await;

    // The start-up pass may have landed before the subscription: the
    // snapshot is the state then, and an event follows if it had not.
    let history: HistorySummary = snapshot(client.subscribe_on(1, HistoryChanged::NAME).await);
    if history.sessions == 0 {
        event_until::<HistorySummary>(&mut client, 1, |summary| summary.sessions == 1).await;
    }
    let stats: ArchiveStats = snapshot(client.subscribe_on(2, ArchiveChanged::NAME).await);
    if stats.sessions == 0 {
        event_until::<ArchiveStats>(&mut client, 2, |stats| stats.sessions == 1).await;
    }
    let summary = client.request::<ReadHistorySummary>(&()).await.unwrap();
    assert_eq!((summary.sessions, summary.prompts), (1, 1));
    let conversation = client
        .request::<ArchiveConversation>(&HistorySessionRef {
            session_id: ONE.to_owned(),
        })
        .await
        .unwrap()
        .expect("archived on the start-up pass");
    assert_eq!(conversation.message_count, 2);
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_conversation_that_ends_in_the_host_is_indexed_and_archived_without_a_refresh() {
    let world = Claude::new();
    let project = world.project("alpha");
    let config = world.hosting_config();
    let endpoint = config.endpoint.clone();
    let host = RunningHost::start(config);
    let mut client = TestClient::connect(&endpoint, &["terminal"]).await;
    let start: HistorySummary = snapshot(client.subscribe_on(1, HistoryChanged::NAME).await);
    assert_eq!(start.sessions, 0);

    let session = client
        .request::<SessionStart>(&StartSessionRequest {
            backend: Some(BackendId::claude_code()),
            cwd: project.display().to_string(),
            cols: 80,
            rows: 24,
            ..StartSessionRequest::default()
        })
        .await
        .unwrap();
    client.attach(2, session.id).await;
    client.type_into(2, b"plan the release\r/exit\r").await;

    // No refresh is asked for: the session's end is what indexes it.
    let summary =
        event_until::<HistorySummary>(&mut client, 1, |summary| summary.sessions == 1).await;
    assert_eq!(
        (summary.sessions, summary.prompts, summary.resumable),
        (1, 1, 1)
    );
    let page = client
        .request::<HistorySessions>(&HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(page.sessions[0].first_prompt, "plan the release");
    let conversation = client
        .request::<ArchiveConversation>(&HistorySessionRef {
            session_id: page.sessions[0].session_id.clone(),
        })
        .await
        .unwrap()
        .expect("archived by the same pass");
    assert_eq!(
        conversation
            .messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>(),
        ["plan the release", "You said: plan the release"]
    );
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_gets_the_history_index_through_the_host_and_every_subscriber_each_change() {
    let world = Claude::new();
    let alpha = world.project("repos/alpha");
    let beta = world.project("repos/beta");
    world.transcript(
        "a",
        ONE,
        &alpha,
        &[
            ("user", &uuid(1), T0, "add geofencing to the map"),
            ("user", &uuid(2), T0 + 1, "now write the tests"),
        ],
    );
    let config = world.config();
    let endpoint = config.endpoint.clone();
    let host = RunningHost::start(config);
    let mut client = TestClient::connect(&endpoint, &[]).await;
    let mut watcher = TestClient::connect(&endpoint, &[]).await;
    let first = client.request::<HistoryRefresh>(&()).await.unwrap();
    assert_eq!(first.sessions, 1);
    let start: HistorySummary = snapshot(watcher.subscribe_on(7, HistoryChanged::NAME).await);
    assert_eq!(start, first);

    // A second conversation appears; a refresh indexes it and the watcher
    // is told.
    world.transcript(
        "b",
        TWO,
        &beta,
        &[("user", &uuid(3), T0 + 10, "fix the geofence radius")],
    );
    let refreshed = client.request::<HistoryRefresh>(&()).await.unwrap();
    assert_eq!(
        (refreshed.sessions, refreshed.prompts, refreshed.projects),
        (2, 3, 2)
    );
    let told: HistorySummary = from_payload(&event_on(&mut watcher, 7).await).unwrap();
    assert_eq!(told, refreshed);

    // Paged, searched, prompts, projects.
    let page = client
        .request::<HistorySessions>(&HistoryQuery {
            limit: Some(1),
            ..HistoryQuery::default()
        })
        .await
        .unwrap();
    assert_eq!((page.sessions.len(), page.total), (1, 2));
    assert_eq!(page.sessions[0].session_id, TWO);
    let found = client
        .request::<HistorySessions>(&HistoryQuery {
            search: Some("WRITE THE".to_owned()),
            ..HistoryQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(ids(&found), [ONE]);
    assert_eq!(
        found.sessions[0].matched.as_deref(),
        Some("now write the tests")
    );
    let prompts = client
        .request::<HistoryPrompts>(&HistorySessionRef {
            session_id: ONE.to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(
        prompts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
        ["add geofencing to the map", "now write the tests"]
    );
    let projects = client.request::<HistoryProjects>(&()).await.unwrap();
    assert_eq!(
        projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["beta", "alpha"]
    );

    // A rename is answered, told to the watcher, and kept across a refresh.
    let renamed = client
        .request::<HistoryRename>(&RenameHistorySessionRequest {
            session_id: ONE.to_owned(),
            name: Some("Geofencing".to_owned()),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(renamed.label.as_deref(), Some("Geofencing"));
    event_on(&mut watcher, 7).await;
    client.request::<HistoryRefresh>(&()).await.unwrap();
    let after = client
        .request::<HistorySessions>(&HistoryQuery::default())
        .await
        .unwrap();
    let one = after.sessions.iter().find(|s| s.session_id == ONE).unwrap();
    assert_eq!(one.label.as_deref(), Some("Geofencing"));
    assert_eq!(
        client
            .request::<HistoryRename>(&RenameHistorySessionRequest {
                session_id: "not-a-session".to_owned(),
                name: Some("x".to_owned()),
            })
            .await
            .unwrap(),
        None
    );
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_oldest_conversations_are_evicted_to_stay_under_the_ceiling_and_subscribers_are_told() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    for (n, id) in [ONE, TWO].iter().enumerate() {
        let text = incompressible(n as u64, 4000);
        world.transcript(
            &format!("p{n}"),
            id,
            &alpha,
            &[("user", &uuid(n as u64 + 1), T0 + n as u64, &text)],
        );
    }
    let config = world.config();
    let endpoint = config.endpoint.clone();
    let host = RunningHost::start(config);
    let mut client = TestClient::connect(&endpoint, &[]).await;
    client.request::<HistoryRefresh>(&()).await.unwrap();
    let full = client.request::<ReadArchiveStats>(&()).await.unwrap();
    assert_eq!(full.sessions, 2);
    let mut watcher = TestClient::connect(&endpoint, &[]).await;
    let start: ArchiveStats = snapshot(watcher.subscribe_on(3, ArchiveChanged::NAME).await);
    assert_eq!(start, full);

    // The ceiling is an ordinary setting, written beside the host.
    let ceiling = full.stored_bytes - 1;
    let db = rusqlite::Connection::open(world.dirs.data.join("floti.db")).unwrap();
    db.execute(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)",
        rusqlite::params![archive::CEILING_SETTING, ceiling.to_string()],
    )
    .unwrap();
    drop(db);
    client.request::<HistoryRefresh>(&()).await.unwrap();
    let told: ArchiveStats = from_payload(&event_on(&mut watcher, 3).await).unwrap();
    assert_eq!(
        (told.sessions, told.evicted_sessions, told.max_bytes),
        (1, 1, ceiling)
    );
    assert!(told.stored_bytes <= ceiling);
    let dropped = client
        .request::<ArchiveConversation>(&HistorySessionRef {
            session_id: ONE.to_owned(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dropped.state, ArchiveSessionState::Evicted);
    assert!(dropped.messages.is_empty());
    host.stop(HostStopReason::Stop).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_with_no_backend_that_keeps_history_answers_with_an_empty_index() {
    let dirs = TestDirs::new();
    let shell = floti_host::server::shell_backend(&dirs.user_env());
    let endpoint = dirs.endpoint();
    let host = RunningHost::start(HostConfig {
        run: Default::default(),
        folders: dirs.folders(),
        endpoint: endpoint.clone(),
        backends: vec![Arc::new(shell) as Arc<dyn AgentBackend>],
        inherited_env: Vec::new(),
    });
    let mut client = TestClient::connect(&endpoint, &[]).await;
    let summary = client.request::<HistoryRefresh>(&()).await.unwrap();
    assert_eq!(
        (summary.sessions, summary.prompts, summary.error),
        (0, 0, None)
    );
    let page = client
        .request::<HistorySessions>(&HistoryQuery::default())
        .await
        .unwrap();
    assert!(page.sessions.is_empty());
    host.stop(HostStopReason::Stop).await;
}
