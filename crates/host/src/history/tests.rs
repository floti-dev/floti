use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use floti_protocol::BackendId;
use floti_protocol::channels::sessions::{
    ArchiveRole, ArchiveSessionState, ArchiveStats, HistorySession,
};
use serde_json::json;

use super::index::HistoryIndex;
use super::{archive, directory_exists, index_history, read_indexed};
use crate::Result;
use crate::agent::claude_code::{ClaudeCodeBackend, ClaudeCodeSettings};
use crate::agent::{AgentBackend, Conversation, HistorySource, Transcript};
use crate::platform::process_tree::Native as NativeTree;
use crate::store::Store;
use crate::testkit::TestDirs;

// ---------------------------------------------------------------------------
// A pass over Claude Code's own history
// ---------------------------------------------------------------------------

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

    fn open_store(&self) -> Store {
        Store::open(&self.dirs.data).expect("open the store")
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

/// Claude Code's transcripts without its prompt record: a backend that keeps
/// none.
struct Unrecorded<'a>(&'a ClaudeCodeBackend);

impl HistorySource for Unrecorded<'_> {
    fn conversations(&self) -> Result<Vec<Conversation>> {
        self.0.history().unwrap().conversations()
    }

    fn transcript(&self, id: &str) -> Result<Option<Transcript>> {
        self.0.history().unwrap().transcript(id)
    }
}

/// A history without a prompt record that counts how often a transcript is
/// read.
struct Counting<'a> {
    inner: &'a ClaudeCodeBackend,
    reads: AtomicUsize,
}

impl HistorySource for Counting<'_> {
    fn conversations(&self) -> Result<Vec<Conversation>> {
        self.inner.history().unwrap().conversations()
    }

    fn transcript(&self, id: &str) -> Result<Option<Transcript>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.history().unwrap().transcript(id)
    }
}

fn pass(store: &Mutex<Store>, source: &dyn HistorySource) -> crate::history::Indexed {
    index_history(
        || store.lock().unwrap(),
        source,
        &BackendId::claude_code(),
        &|| true,
    )
    .expect("a pass")
}

/// Every file under `root` with its size and modification time.
fn tree_state(root: &Path) -> BTreeMap<PathBuf, (u64, Option<SystemTime>)> {
    let mut found = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                found.insert(entry.path(), (meta.len(), meta.modified().ok()));
            }
        }
    }
    found
}

#[test]
fn a_pass_indexes_each_conversation_with_its_prompts_and_archives_its_messages() {
    let world = Claude::new();
    let alpha = world.project("repos/alpha");
    world.transcript(
        "-repos-alpha",
        ONE,
        &alpha,
        &[
            ("user", &uuid(1), T0, "add geofencing to the map"),
            ("assistant", &uuid(2), T0 + 1, "done"),
            ("user", &uuid(3), T0 + 2, "now write the tests"),
        ],
    );
    let store = Mutex::new(world.open_store());
    let backend = world.backend();
    let indexed = pass(&store, &backend);
    assert_eq!(
        (
            indexed.summary.sessions,
            indexed.summary.prompts,
            indexed.summary.resumable
        ),
        (1, 2, 1)
    );
    assert_eq!(
        indexed.summary.history_file,
        world.history_file().display().to_string()
    );
    assert_eq!((indexed.stats.sessions, indexed.stats.messages), (1, 3));
    let guard = store.lock().unwrap();
    let id = BackendId::claude_code();
    let index = HistoryIndex::new(guard.connection(), &id);
    let session = index.session(ONE).unwrap().unwrap();
    assert_eq!(session.project, alpha.display().to_string());
    assert_eq!(session.title, "add geofencing to the map");
    assert_eq!(session.archive, Some(ArchiveSessionState::Archived));
    assert_eq!(session.archived_messages, Some(3));
}

#[test]
fn a_transcript_that_has_not_changed_is_not_read_again() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    let file = world.transcript(
        "a",
        ONE,
        &alpha,
        &[("user", &uuid(1), T0, "hello there friend")],
    );
    let store = Mutex::new(world.open_store());
    let backend = world.backend();
    let counting = Counting {
        inner: &backend,
        reads: AtomicUsize::new(0),
    };
    pass(&store, &counting);
    pass(&store, &counting);
    assert_eq!(counting.reads.load(Ordering::SeqCst), 1);

    // It grew: read again, and only what it grew by is added.
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str(&transcript_row(
        "user",
        &uuid(2),
        &alpha,
        T0 + 5,
        "and one more thing",
    ));
    std::fs::write(&file, text).unwrap();
    let indexed = pass(&store, &counting);
    assert_eq!(counting.reads.load(Ordering::SeqCst), 2);
    assert_eq!(indexed.stats.messages, 2);
    assert_eq!(indexed.summary.prompts, 2);
}

#[test]
fn a_transcript_that_was_replaced_is_read_again_and_keeps_what_was_archived() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    world.transcript(
        "a",
        ONE,
        &alpha,
        &[
            ("user", &uuid(1), T0, "one"),
            ("user", &uuid(2), T0 + 1, "two"),
            ("user", &uuid(3), T0 + 2, "three"),
        ],
    );
    let store = Mutex::new(world.open_store());
    let backend = world.backend();
    let source = Unrecorded(&backend);
    pass(&store, &source);
    world.transcript("a", ONE, &alpha, &[("user", &uuid(4), T0 + 3, "four")]);
    let indexed = pass(&store, &source);
    // The archive keeps every message it had; without a prompt record the
    // index lists what the transcript says now.
    assert_eq!(indexed.stats.messages, 4);
    assert_eq!(indexed.summary.prompts, 1);
}

/// The prompts the index holds for `id`, in submission order.
fn prompts_of(store: &Mutex<Store>, id: &str) -> Vec<String> {
    let guard = store.lock().unwrap();
    let backend = BackendId::claude_code();
    HistoryIndex::new(guard.connection(), &backend)
        .prompts(id)
        .unwrap()
        .into_iter()
        .map(|prompt| prompt.text)
        .collect()
}

fn session_of(store: &Mutex<Store>, id: &str) -> Option<HistorySession> {
    let guard = store.lock().unwrap();
    let backend = BackendId::claude_code();
    HistoryIndex::new(guard.connection(), &backend)
        .session(id)
        .unwrap()
}

#[test]
fn every_conversation_the_prompt_record_names_is_listed_with_its_prompts_even_once_its_transcript_is_reaped()
 {
    let world = Claude::new();
    let alpha = world.project("alpha");
    let beta = world.project("beta");
    // ONE still has its transcript; TWO's was reaped before the host saw it.
    world.transcript(
        "a",
        ONE,
        &alpha,
        &[
            ("user", &uuid(1), T0, "first prompt here"),
            ("assistant", &uuid(2), T0 + 1, "an answer"),
            ("user", &uuid(3), T0 + 2, "second prompt here"),
        ],
    );
    world.prompt(TWO, &beta, T0 + 10, "a conversation long gone");
    world.prompt(TWO, &beta, T0 + 11, "");
    world.prompt(TWO, &beta, T0 + 12, "and its last prompt");
    let store = Mutex::new(world.open_store());
    let backend = world.backend();

    let indexed = pass(&store, &backend);
    assert_eq!(
        (
            indexed.summary.sessions,
            indexed.summary.prompts,
            indexed.summary.resumable
        ),
        (2, 5, 1)
    );
    assert_eq!(indexed.summary.error, None);
    assert_eq!(
        prompts_of(&store, ONE),
        ["first prompt here", "second prompt here"]
    );
    assert_eq!(
        prompts_of(&store, TWO),
        ["a conversation long gone", "", "and its last prompt"]
    );
    let gone = session_of(&store, TWO).expect("listed from the record");
    assert_eq!(gone.project, beta.display().to_string());
    assert_eq!(gone.transcript_file, None, "not resumable");
    assert!(session_of(&store, ONE).unwrap().transcript_file.is_some());

    // ONE's transcript is reaped too: it stays listed, no longer resumable,
    // and its messages are still in the archive.
    std::fs::remove_dir_all(world.config.join("projects").join("a")).unwrap();
    let after = pass(&store, &backend);
    assert_eq!((after.summary.sessions, after.summary.resumable), (2, 0));
    assert_eq!(
        prompts_of(&store, ONE),
        ["first prompt here", "second prompt here"]
    );
    assert_eq!(session_of(&store, ONE).unwrap().transcript_file, None);
    assert_eq!(after.stats.messages, 3);
}

#[test]
fn each_pass_reads_only_what_the_prompt_record_gained_and_starts_over_when_it_shrank_or_went() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    world.prompt(ONE, &alpha, T0, "first");
    let store = Mutex::new(world.open_store());
    let backend = world.backend();
    let first = pass(&store, &backend);
    let size = std::fs::metadata(world.history_file()).unwrap().len();
    assert_eq!(first.summary.indexed_bytes, size);

    // Grown, with a half-written last line and a malformed one: the complete
    // lines are added once, the fragment waits for the next pass.
    world.prompt(ONE, &alpha, T0 + 1, "second ελληνικά 🚀");
    world.append_history("not json at all\n");
    let fragment = history_line(TWO, &alpha, T0 + 2, "third");
    let (head, rest) = fragment.split_at(fragment.find("third").unwrap() + 2);
    world.append_history(head);
    let grown = pass(&store, &backend);
    assert_eq!(prompts_of(&store, ONE), ["first", "second ελληνικά 🚀"]);
    assert_eq!(grown.summary.sessions, 1);
    assert_eq!(
        grown.summary.indexed_bytes,
        std::fs::metadata(world.history_file()).unwrap().len() - head.len() as u64
    );
    world.append_history(rest);
    let finished = pass(&store, &backend);
    assert_eq!(finished.summary.sessions, 2);
    assert_eq!(prompts_of(&store, TWO), ["third"]);
    assert_eq!(finished.summary.prompts, 3);

    // Nothing new: nothing added.
    assert_eq!(pass(&store, &backend).summary.prompts, 3);

    // Shrunk: a different file, so the index starts over from it.
    std::fs::write(
        world.history_file(),
        history_line(TWO, &alpha, T0 + 20, "only one now"),
    )
    .unwrap();
    let shrunk = pass(&store, &backend);
    assert_eq!((shrunk.summary.sessions, shrunk.summary.prompts), (1, 1));
    assert!(session_of(&store, ONE).is_none());
    assert_eq!(prompts_of(&store, TWO), ["only one now"]);

    // Gone: the index starts over with nothing, and says why.
    std::fs::remove_file(world.history_file()).unwrap();
    let gone = pass(&store, &backend);
    assert_eq!((gone.summary.sessions, gone.summary.prompts), (0, 0));
    assert_eq!(gone.summary.indexed_bytes, 0);
    assert!(gone.summary.error.is_some());
}

#[test]
fn a_backend_that_keeps_no_prompt_record_has_its_sessions_and_prompts_from_its_transcripts() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    world.transcript(
        "a",
        ONE,
        &alpha,
        &[
            ("user", &uuid(1), T0, "from the transcript"),
            ("assistant", &uuid(2), T0 + 1, "an answer"),
            ("user", &uuid(3), T0 + 2, "and again"),
        ],
    );
    // A record the backend does not say it keeps is not read.
    world.prompt(TWO, &alpha, T0 + 5, "only in a record nobody reads");
    let store = Mutex::new(world.open_store());
    let backend = world.backend();
    let indexed = pass(&store, &Unrecorded(&backend));
    assert_eq!(
        (
            indexed.summary.sessions,
            indexed.summary.prompts,
            indexed.summary.resumable
        ),
        (1, 2, 1)
    );
    assert_eq!(
        indexed.summary.history_file,
        world.config.join("projects").display().to_string()
    );
    assert_eq!(
        prompts_of(&store, ONE),
        ["from the transcript", "and again"]
    );
    assert!(session_of(&store, TWO).is_none());
}

#[test]
fn the_agents_folder_is_left_byte_for_byte_as_it_was_on_a_full_pass_from_cleared_cursors() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    world.transcript(
        "a",
        ONE,
        &alpha,
        &[("user", &uuid(1), T0, "hello there friend")],
    );
    world.transcript(
        "b",
        TWO,
        &alpha,
        &[("user", &uuid(2), T0 + 1, "second conversation here")],
    );
    std::fs::write(world.config.join("history.jsonl"), "{}\n").unwrap();
    let before = tree_state(&world.config);
    let store = Mutex::new(world.open_store());
    let backend = world.backend();
    pass(&store, &backend);
    store
        .lock()
        .unwrap()
        .connection()
        .execute("DELETE FROM transcript_index", [])
        .unwrap();
    pass(&store, &backend);
    assert_eq!(tree_state(&world.config), before);
}

#[test]
fn a_ceiling_written_in_the_settings_is_honoured_on_the_next_pass() {
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
    let store = Mutex::new(world.open_store());
    let backend = world.backend();
    let first = pass(&store, &backend);
    assert_eq!(first.stats.sessions, 2);
    assert_eq!(first.stats.max_bytes, archive::DEFAULT_CEILING);

    let ceiling = first.stats.stored_bytes - 1;
    store
        .lock()
        .unwrap()
        .connection()
        .execute(
            "INSERT INTO app_settings (key, value) VALUES (?, ?)",
            rusqlite::params![archive::CEILING_SETTING, ceiling.to_string()],
        )
        .unwrap();
    let next = pass(&store, &backend);
    assert_eq!(next.stats.max_bytes, ceiling);
    assert_eq!((next.stats.sessions, next.stats.evicted_sessions), (1, 1));
    assert!(next.stats.stored_bytes <= ceiling);
    // The older one went.
    let guard = store.lock().unwrap();
    assert_eq!(
        archive::state_of(guard.connection(), ONE).unwrap(),
        Some(ArchiveSessionState::Evicted)
    );
}

#[test]
fn a_conversation_whose_transcript_was_deleted_is_kept_across_a_restart_identical_and_searchable() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    let file = world.transcript(
        "a",
        ONE,
        &alpha,
        &[
            ("user", &uuid(1), T0, "where is the zorblatt configured"),
            ("assistant", &uuid(2), T0 + 1, "in the settings pane"),
        ],
    );
    let backend = world.backend();
    let before = {
        let store = Mutex::new(world.open_store());
        pass(&store, &backend);
        let guard = store.lock().unwrap();
        archive::read_conversation(guard.connection(), ONE)
            .unwrap()
            .unwrap()
    };
    std::fs::remove_file(&file).unwrap();

    let store = Mutex::new(world.open_store());
    let indexed = pass(&store, &backend);
    assert_eq!(indexed.summary.resumable, 0);
    let guard = store.lock().unwrap();
    let after = archive::read_conversation(guard.connection(), ONE)
        .unwrap()
        .unwrap();
    assert_eq!(after, before);
    assert_eq!(after.state, ArchiveSessionState::Archived);
    assert_eq!(
        after
            .messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>(),
        ["where is the zorblatt configured", "in the settings pane"]
    );
    assert_eq!(after.source_file, file.display().to_string());
    assert_eq!(
        archive::read_conversation(guard.connection(), TWO).unwrap(),
        None
    );
    let stats = archive::read_stats(guard.connection(), archive::DEFAULT_CEILING).unwrap();
    assert_eq!((stats.sessions, stats.messages), (1, 2));
    assert_eq!(stats.stored_bytes, after.stored_bytes);
    assert!(
        archive::search_archive(guard.connection(), "zorblatt", 10)
            .unwrap()
            .contains_key(ONE)
    );
    let id = BackendId::claude_code();
    let session = HistoryIndex::new(guard.connection(), &id)
        .session(ONE)
        .unwrap()
        .unwrap();
    assert_eq!(session.transcript_file, None);
    assert_eq!(session.prompt_count, 1);
}

#[test]
fn a_pass_told_to_stop_writes_nothing_more_and_the_next_carries_on_from_there() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    world.transcript(
        "a",
        ONE,
        &alpha,
        &[("user", &uuid(1), T0, "first conversation here")],
    );
    world.transcript(
        "b",
        TWO,
        &alpha,
        &[("user", &uuid(2), T0 + 1, "second conversation here")],
    );
    let store = Mutex::new(world.open_store());
    let backend = world.backend();
    let source = backend.history().unwrap();
    let id = BackendId::claude_code();

    let stopped = index_history(|| store.lock().unwrap(), source, &id, &|| false).unwrap();
    assert_eq!((stopped.summary.sessions, stopped.stats.messages), (0, 0));

    // Stopped between the two transcripts' messages: one is archived.
    let asked = AtomicUsize::new(0);
    let partway = index_history(|| store.lock().unwrap(), source, &id, &|| {
        asked.fetch_add(1, Ordering::SeqCst) < 3
    })
    .unwrap();
    assert_eq!(partway.summary.sessions, 2);
    assert_eq!(partway.stats.messages, 1);

    let finished = pass(&store, &backend);
    assert_eq!((finished.summary.sessions, finished.stats.sessions), (2, 2));
    assert_eq!(finished.stats.messages, 2);
}

#[test]
fn the_tree_the_config_folder_names_is_read_and_not_the_one_in_the_home() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    // A tree in the default place that the backend was not pointed at.
    let elsewhere = world.dirs.folder("elsewhere/.claude");
    let dir = elsewhere.join("projects").join("x");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{TWO}.jsonl")),
        transcript_row("user", &uuid(9), &alpha, T0, "not this one please"),
    )
    .unwrap();
    world.transcript(
        "a",
        ONE,
        &alpha,
        &[("user", &uuid(1), T0, "this one is right")],
    );
    let store = Mutex::new(world.open_store());
    let indexed = pass(&store, &world.backend());
    assert_eq!(indexed.summary.sessions, 1);
    let guard = store.lock().unwrap();
    let id = BackendId::claude_code();
    assert!(
        HistoryIndex::new(guard.connection(), &id)
            .session(TWO)
            .unwrap()
            .is_none()
    );
}

/// A recorded folder counts as there only while it is a directory.
#[test]
fn a_recorded_folder_exists_when_it_is_a_directory_and_not_when_it_is_a_file_or_missing() {
    let dirs = TestDirs::new();
    let dir = dirs.folder("recorded");
    std::fs::write(dir.join("file.txt"), "x").unwrap();
    let text = |path: PathBuf| path.display().to_string();
    assert!(directory_exists(&text(dir.clone())));
    assert!(!directory_exists(&text(dir.join("file.txt"))));
    assert!(!directory_exists(&text(dir.join("gone"))));
}

#[test]
fn what_is_indexed_reads_without_a_pass() {
    let world = Claude::new();
    let store = world.open_store();
    let indexed = read_indexed(store.connection(), &BackendId::claude_code()).unwrap();
    assert_eq!(indexed.summary.sessions, 0);
    assert_eq!(
        indexed.stats,
        ArchiveStats {
            sessions: 0,
            evicted_sessions: 0,
            messages: 0,
            raw_bytes: 0,
            stored_bytes: 0,
            max_bytes: archive::DEFAULT_CEILING,
            oldest_at: None,
            newest_at: None,
        }
    );
}

#[test]
fn a_turn_that_ends_on_thinking_never_finished_and_its_thinking_is_not_the_last_reply() {
    let world = Claude::new();
    let alpha = world.project("alpha");
    // Escape pressed while the agent was thinking: Claude Code writes the
    // thinking as a row of its own, then the interruption as the user's.
    world.transcript(
        "a",
        ONE,
        &alpha,
        &[
            ("user", &uuid(1), T0, "a question"),
            ("thinking", &uuid(2), T0 + 1, "Weighing the watcher first."),
            ("assistant", &uuid(3), T0 + 2, "Fixed the watcher."),
            ("user", &uuid(4), T0 + 3, "and the tests?"),
            (
                "thinking",
                &uuid(5),
                T0 + 4,
                "The user wants the tests run.",
            ),
            ("user", &uuid(6), T0 + 5, "[Request interrupted by user]"),
        ],
    );
    // Lost while thinking: nothing after it.
    world.transcript(
        "b",
        TWO,
        &alpha,
        &[
            ("user", &uuid(7), T0, "a question"),
            ("assistant", &uuid(8), T0 + 1, "Done."),
            ("user", &uuid(9), T0 + 2, "one more thing"),
            (
                "thinking",
                &uuid(10),
                T0 + 3,
                "Considering the one more thing.",
            ),
        ],
    );
    let store = Mutex::new(world.open_store());
    pass(&store, &world.backend());
    let store = store.lock().unwrap();
    let reply = |id: &str| archive::last_reply(store.connection(), id).unwrap();
    assert_eq!(reply(ONE).as_deref(), Some("Fixed the watcher."));
    assert_eq!(reply(TWO).as_deref(), Some("Done."));
    // The archive still holds the thinking, marked as such.
    let archived = archive::read_conversation(store.connection(), TWO)
        .unwrap()
        .expect("archived");
    let thinking: Vec<(&str, bool)> = archived
        .messages
        .iter()
        .map(|message| (message.text.as_str(), message.thinking))
        .collect();
    assert_eq!(
        thinking,
        [
            ("a question", false),
            ("Done.", false),
            ("one more thing", false),
            ("Considering the one more thing.", true),
        ]
    );
    assert_eq!(archived.messages[3].role, ArchiveRole::Assistant);
}
