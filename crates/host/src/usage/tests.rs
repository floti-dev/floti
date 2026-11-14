//! The host's readings: which backends are read, when a reading is a change,
//! the spend left out where the day cannot be told, and the kept usage read
//! from the store only when it changed. Claude Code's backend reads a
//! `.claude.json` the test writes, in a home of the test's own; no agent runs.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use floti_protocol::BackendId;
use floti_protocol::channels::sessions::UsageSnapshot;
use jiff::tz::TimeZone;
use serde_json::{Value, json};

use super::{KeptSource, KeptUsage, LocalDay, UsageReader, ZonedDay, now_ms, same_readings};
use crate::agent::AgentBackend;
use crate::agent::claude_code::{ClaudeCodeBackend, ClaudeCodeSettings};
use crate::platform::process_tree::Native as NativeTree;
use crate::server::shell_backend;
use crate::testkit::TestDirs;

/// 2026-08-10T08:55:00Z.
const FETCHED: u64 = 1_786_352_100_000;

const DAY_MS: u64 = 24 * 3_600_000;

/// A home with Claude Code's config folder in it.
struct Claude {
    config: PathBuf,
    /// Last: fields drop in order.
    dirs: TestDirs,
}

impl Claude {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let config = dirs.home.join(".claude");
        std::fs::create_dir_all(config.join("projects")).unwrap();
        Self { config, dirs }
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

    /// Claude Code's backend, which declares usage, and a shell, which does
    /// not.
    fn backends(&self) -> Vec<Arc<dyn AgentBackend>> {
        vec![
            Arc::new(self.backend()) as Arc<dyn AgentBackend>,
            Arc::new(shell_backend(&self.dirs.user_env())),
        ]
    }

    /// Writes `.claude.json` where the CLI keeps it for the default config
    /// folder: beside it.
    fn write(&self, root: &Value) {
        std::fs::write(self.dirs.home.join(".claude.json"), root.to_string()).unwrap();
    }
}

/// `.claude.json` with the three limits a 2.1.225 reading carries, fetched
/// at `fetched_at_ms`.
fn claude_json(fetched_at_ms: u64) -> Value {
    json!({
        "numStartups": 42,
        "projects": {},
        "cachedUsageUtilization": {
            "fetchedAtMs": fetched_at_ms,
            "utilization": { "limits": [
                { "kind": "session", "group": "session", "percent": 51, "severity": "normal",
                  "resets_at": "2026-08-10T13:10:00.721285+00:00", "is_active": true },
                { "kind": "weekly_all", "group": "weekly", "percent": 38, "severity": "warning",
                  "resets_at": "2026-08-11T19:00:00.721310+00:00", "is_active": false },
                { "kind": "weekly_scoped", "group": "weekly", "percent": 21, "severity": "normal",
                  "resets_at": "2026-08-11T18:59:59.721602+00:00",
                  "scope": { "model": { "display_name": "Fable" } }, "is_active": false },
            ]},
        },
    })
}

/// A local day that starts at a fixed instant.
struct FixedDay(u64);

impl LocalDay for FixedDay {
    fn today_start_ms(&self, _now_ms: u64) -> Option<u64> {
        Some(self.0)
    }
}

/// A local day nobody can tell.
struct NoDay;

impl LocalDay for NoDay {
    fn today_start_ms(&self, _now_ms: u64) -> Option<u64> {
        None
    }
}

#[test]
fn a_reading_is_taken_only_from_the_backends_that_declare_usage() {
    let world = Claude::new();
    world.write(&claude_json(FETCHED));
    let reader = UsageReader::new(&world.backends(), Box::new(FixedDay(0)));
    assert!(
        reader.latest().is_empty(),
        "nothing is read before a reading"
    );
    let (readings, changed) = reader.read();
    assert!(changed);
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].backend, BackendId::claude_code());
    assert_eq!(readings[0].limits.len(), 3);
    assert_eq!(reader.latest(), readings);
}

#[test]
fn a_reading_that_says_the_same_again_is_no_change() {
    let world = Claude::new();
    world.write(&claude_json(FETCHED));
    let reader = UsageReader::new(&world.backends(), Box::new(FixedDay(0)));
    reader.read();
    assert!(!reader.read().1);
    world.write(&claude_json(FETCHED + 1));
    assert!(reader.read().1);
}

#[test]
fn the_estimate_is_left_out_where_local_midnight_cannot_be_told() {
    let world = Claude::new();
    world.write(&claude_json(FETCHED));
    let unknown = UsageReader::new(&world.backends(), Box::new(NoDay));
    let (readings, _) = unknown.read();
    assert_eq!(readings[0].spend, None);
    assert_eq!(readings[0].limits.len(), 3);

    let known = UsageReader::new(&world.backends(), Box::new(FixedDay(0)));
    assert!(known.read().0[0].spend.is_some());
}

#[test]
fn a_reading_taken_in_a_zone_nobody_can_tell_leaves_the_spend_out_and_keeps_the_limits() {
    let world = Claude::new();
    world.write(&claude_json(FETCHED));
    let reader = UsageReader::new(
        &world.backends(),
        Box::new(ZonedDay::new(TimeZone::unknown())),
    );
    let (readings, _) = reader.read();
    assert_eq!(readings[0].spend, None);
    assert_eq!(readings[0].limits.len(), 3);
}

#[test]
fn how_long_the_estimate_took_to_sum_is_not_a_change() {
    let world = Claude::new();
    world.write(&claude_json(FETCHED));
    let reader = UsageReader::new(&world.backends(), Box::new(FixedDay(0)));
    let (first, _) = reader.read();
    let mut slower = first.clone();
    if let Some(spend) = &mut slower[0].spend {
        spend.index_ms += 12.5;
    }
    assert!(same_readings(&first, &slower));
    slower[0].limits[0].percent += 1.0;
    assert!(!same_readings(&first, &slower));
}

#[test]
fn a_host_with_no_backend_that_declares_usage_reads_none() {
    let world = Claude::new();
    let shell: Vec<Arc<dyn AgentBackend>> = vec![Arc::new(shell_backend(&world.dirs.user_env()))];
    let reader = UsageReader::new(&shell, Box::new(FixedDay(0)));
    assert_eq!(reader.read(), (Vec::new(), false));
}

/// Usage the store keeps, as a reader is given it, counting how often it is
/// read. A store that cannot be read errs until it is mended.
#[derive(Clone, Default)]
struct KeptStore {
    rows: Arc<Mutex<Vec<KeptUsage>>>,
    reads: Arc<AtomicUsize>,
    broken: Arc<AtomicBool>,
}

impl KeptStore {
    fn source(&self) -> KeptSource {
        let store = self.clone();
        Box::new(move |since| {
            store.reads.fetch_add(1, Ordering::SeqCst);
            if store.broken.load(Ordering::SeqCst) {
                return Err(crate::Error::Refused("the store could not be read".into()));
            }
            Ok(store
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|row| row.at >= since)
                .cloned()
                .collect())
        })
    }

    fn keep(&self, uuid: &str, at: u64) {
        self.rows.lock().unwrap().push(KeptUsage {
            uuid: uuid.to_owned(),
            at,
            model: "claude-opus-5".to_owned(),
            input: 1000,
            output: 100,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_read: 0,
        });
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

fn week_messages(readings: &[UsageSnapshot]) -> Option<u32> {
    readings[0].spend.as_ref().map(|spend| spend.week.messages)
}

#[test]
fn kept_usage_is_read_from_the_store_once_however_many_readings_follow_the_agents_figures() {
    let world = Claude::new();
    let now = now_ms();
    let store = KeptStore::default();
    store.keep("kept-1", now - DAY_MS);
    // Older than the longest window: never counted.
    store.keep("kept-old", now - 9 * DAY_MS);
    let reader = UsageReader::new(&world.backends(), Box::new(FixedDay(0))).keeping(store.source());

    for step in 0..50 {
        world.write(&claude_json(FETCHED + step));
        let (readings, changed) = reader.read();
        assert!(changed, "reading {step} follows the agent's figures");
        assert_eq!(readings[0].fetched_at_ms, Some(FETCHED + step));
        assert_eq!(week_messages(&readings), Some(1));
    }
    assert_eq!(store.reads(), 1);
}

#[test]
fn kept_usage_the_store_gains_is_read_once_more_and_counted() {
    let world = Claude::new();
    world.write(&claude_json(FETCHED));
    let now = now_ms();
    let store = KeptStore::default();
    store.keep("kept-1", now - DAY_MS);
    let reader = UsageReader::new(&world.backends(), Box::new(FixedDay(0))).keeping(store.source());
    assert_eq!(week_messages(&reader.read().0), Some(1));

    // Rows land in the store; until it is said, the reading is the one before.
    store.keep("copied-1", now - 2 * DAY_MS);
    store.keep("copied-2", now - 3 * DAY_MS);
    assert!(!reader.read().1);
    assert_eq!(store.reads(), 1);

    reader.kept_changed();
    let (readings, changed) = reader.read();
    assert!(changed);
    assert_eq!(week_messages(&readings), Some(3));
    assert_eq!(store.reads(), 2);
    reader.read();
    assert_eq!(store.reads(), 2);
}

#[test]
fn kept_usage_that_cannot_be_read_leaves_the_spend_unknown_and_is_tried_again_until_it_reads() {
    let world = Claude::new();
    world.write(&claude_json(FETCHED));
    let store = KeptStore::default();
    store.keep("kept-1", now_ms() - DAY_MS);
    store.broken.store(true, Ordering::SeqCst);
    let reader = UsageReader::new(&world.backends(), Box::new(FixedDay(0))).keeping(store.source());

    for tries in 1..=3 {
        let (readings, _) = reader.read();
        assert_eq!(readings[0].spend, None, "unknown, never zero");
        assert_eq!(readings[0].limits.len(), 3);
        assert_eq!(store.reads(), tries);
    }

    store.broken.store(false, Ordering::SeqCst);
    assert_eq!(week_messages(&reader.read().0), Some(1));
    assert_eq!(store.reads(), 4);
    reader.read();
    assert_eq!(store.reads(), 4);
}
