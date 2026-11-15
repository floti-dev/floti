//! Usage through a real host on its socket: no backend reports usage, so a
//! client asking for it, or watching it, is given no readings, even with
//! Claude Code's own cached figures on disk.
//!
//! Every folder is temporary: a home with the agent's config folder in it,
//! and the host's data folder. No agent runs; `.claude.json` is written as
//! Claude Code writes it.

use std::sync::Arc;

use floti_host::agent::AgentBackend;
use floti_host::agent::claude_code::{ClaudeCodeBackend, ClaudeCodeSettings};
use floti_host::platform::process_tree::Native as NativeTree;
use floti_host::server::HostConfig;
use floti_host::testkit::{RunningHost, TestClient, TestDirs};
use floti_protocol::channel::from_payload;
use floti_protocol::channels::host::HostStopReason;
use floti_protocol::channels::sessions::{ReadUsage, UsageChanged, UsageSnapshot};
use floti_protocol::{BackendFeature, EventChannel, Message};
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn a_host_running_claude_code_answers_usage_with_no_readings() {
    let dirs = TestDirs::new();
    let config_dir = dirs.home.join(".claude");
    std::fs::create_dir_all(config_dir.join("projects")).unwrap();
    // Figures Claude Code cached, which the host does not read.
    let cached = json!({
        "numStartups": 42,
        "projects": {},
        "cachedUsageUtilization": {
            "fetchedAtMs": 1_786_000_000_000_u64,
            "utilization": {
                "limits": [
                    { "kind": "session", "group": "session", "percent": 51, "resets_at": null },
                ],
            },
        },
    });
    std::fs::write(dirs.home.join(".claude.json"), cached.to_string()).unwrap();
    let backend = ClaudeCodeBackend::new(
        ClaudeCodeSettings {
            search_first: Vec::new(),
            program: "claude".into(),
            leading_args: Vec::new(),
            config_dir,
            pass_config_dir: true,
            api_key_set: false,
        },
        Arc::new(NativeTree),
    );
    assert!(
        !backend
            .capabilities()
            .features
            .contains(&BackendFeature::Usage)
    );
    let endpoint = dirs.endpoint();
    let host = RunningHost::start(HostConfig {
        run: Default::default(),
        folders: dirs.folders(),
        endpoint: endpoint.clone(),
        backends: vec![Arc::new(backend) as Arc<dyn AgentBackend>],
        inherited_env: Vec::new(),
    });
    let mut client = TestClient::connect(&endpoint, &[]).await;

    assert!(client.request::<ReadUsage>(&()).await.unwrap().is_empty());
    let watched: Vec<UsageSnapshot> = match client.subscribe_on(4, UsageChanged::NAME).await {
        Message::Snapshot(snapshot) => from_payload(&snapshot.state).unwrap(),
        other => panic!("expected a snapshot, got {other:?}"),
    };
    assert!(watched.is_empty());
    host.stop(HostStopReason::Stop).await;
}
