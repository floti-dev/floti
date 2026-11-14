use std::collections::{BTreeMap, BTreeSet};

use floti_protocol::BackendId;
use floti_protocol::channels::sessions::{
    ArchiveMessage, ArchiveRole, ArchiveSessionState, HistoryQuery, HistorySearchScope,
};

use super::{ArchiveInput, search_query};
use crate::history::archive;
use crate::history::index::{HistoryIndex, IndexInput, PromptLine};
use crate::store::Store;
use crate::testkit::TestDirs;

/// The record the archive tests feed, as a path on any OS.
const RECORD: &str = "history.jsonl";

#[test]
fn a_search_is_quoted_so_fts_syntax_cannot_leak_out_of_the_box() {
    assert_eq!(
        search_query("hello world").as_deref(),
        Some("\"hello\" AND \"world\"*")
    );
    assert_eq!(
        search_query("-not \"a phrase\"").as_deref(),
        Some("\"not\" AND \"a\" AND \"phrase\"*")
    );
    assert_eq!(search_query("   "), None);
}

// ---------------------------------------------------------------------------
// The archive on its own
// ---------------------------------------------------------------------------

struct Archive {
    store: Store,
    next_uuid: u64,
    /// Last: fields drop in order, and the folder goes only once nothing in it is open.
    _dirs: TestDirs,
}

impl Archive {
    fn new() -> Self {
        let dirs = TestDirs::new();
        Self {
            store: Store::open(&dirs.data).expect("open the store"),
            _dirs: dirs,
            next_uuid: 0,
        }
    }

    fn conn(&self) -> &rusqlite::Connection {
        self.store.connection()
    }

    fn message(&mut self, role: ArchiveRole, at: u64, text: &str) -> ArchiveMessage {
        self.next_uuid += 1;
        ArchiveMessage {
            uuid: format!("00000000-0000-4000-8000-{:012x}", self.next_uuid),
            role,
            at: 1_760_000_000_000 + at,
            text: text.to_owned(),
            thinking: false,
        }
    }

    /// One of the agent's messages that is only its thinking, as the
    /// backend marks it.
    fn thinking(&mut self, at: u64, text: &str) -> ArchiveMessage {
        ArchiveMessage {
            thinking: true,
            ..self.message(ArchiveRole::Assistant, at, text)
        }
    }

    fn user(&mut self, at: u64, text: &str) -> ArchiveMessage {
        self.message(ArchiveRole::User, at, text)
    }

    fn archive(
        &self,
        file: &str,
        session: &str,
        messages: &[ArchiveMessage],
    ) -> archive::ArchiveWrite {
        archive::archive_transcript(
            self.conn(),
            &ArchiveInput {
                file,
                session_id: session,
                bytes: messages.len() as u64 * 100,
                messages,
            },
        )
        .expect("archive a transcript")
    }

    fn texts(&self, session: &str) -> Vec<String> {
        archive::read_conversation(self.conn(), session)
            .unwrap()
            .unwrap()
            .messages
            .into_iter()
            .map(|message| message.text)
            .collect()
    }

    /// Four sessions, oldest first, each big enough to be worth evicting.
    fn fill(&mut self) {
        for (index, name) in ["oldest", "older", "newer", "newest"].iter().enumerate() {
            let text = format!("conversation {name} ").repeat(60);
            let message = self.user(index as u64 * 1000, &text);
            self.archive(&format!("{name}.jsonl"), name, &[message]);
        }
    }

    fn bytes(&self) -> u64 {
        archive::archived_bytes(self.conn()).unwrap()
    }

    fn state(&self, session: &str) -> Option<ArchiveSessionState> {
        archive::state_of(self.conn(), session).unwrap()
    }

    fn search(&self, search: &str) -> BTreeMap<String, String> {
        archive::search_archive(self.conn(), search, 2000).unwrap()
    }
}

#[test]
fn a_transcript_offered_again_adds_exactly_the_messages_it_grew_by() {
    let mut world = Archive::new();
    let first = vec![
        world.user(1, "hello"),
        world.message(ArchiveRole::Assistant, 2, "one"),
    ];
    assert_eq!(world.archive("grown.jsonl", "sess", &first).messages, 2);
    let mut grown = first.clone();
    grown.push(world.user(3, "two"));
    grown.push(world.message(ArchiveRole::Assistant, 4, "three"));
    assert_eq!(world.archive("grown.jsonl", "sess", &grown).messages, 2);
    assert_eq!(world.texts("sess"), ["hello", "one", "two", "three"]);
    assert_eq!(
        archive::cursor(world.conn(), "grown.jsonl").unwrap(),
        Some(400)
    );
}

#[test]
fn a_message_a_fork_put_in_a_second_transcript_is_counted_once() {
    let mut world = Archive::new();
    let shared = world.user(1, "hello");
    let parent = [shared.clone(), world.user(2, "parent")];
    world.archive("parent.jsonl", "sess", &parent);
    let fork = [shared, world.user(3, "fork")];
    assert_eq!(world.archive("fork.jsonl", "sess", &fork).messages, 1);
    let conversation = archive::read_conversation(world.conn(), "sess")
        .unwrap()
        .unwrap();
    assert_eq!(conversation.message_count, 3);
}

#[test]
fn a_cursor_for_a_transcript_that_has_gone_is_forgotten_and_its_conversation_kept() {
    let mut world = Archive::new();
    let messages = [world.user(1, "hello")];
    world.archive("reaped.jsonl", "sess", &messages);
    assert!(
        archive::indexed_files(world.conn())
            .unwrap()
            .contains_key("reaped.jsonl")
    );
    let gone = ["reaped.jsonl".to_owned()];
    assert_eq!(archive::forget_files(world.conn(), &gone).unwrap(), 1);
    assert!(
        !archive::indexed_files(world.conn())
            .unwrap()
            .contains_key("reaped.jsonl")
    );
    assert_eq!(world.texts("sess").len(), 1);
}

#[test]
fn a_long_body_is_stored_compressed_a_short_one_as_it_is_and_both_come_back_byte_for_byte() {
    let mut world = Archive::new();
    let long = "the quick brown fox jumps over the lazy dog. ".repeat(40);
    let messages = [world.user(1, "short"), world.user(2, &long)];
    world.archive("sizes.jsonl", "sess", &messages);
    let rows: Vec<(i64, i64, i64)> = world
        .conn()
        .prepare("SELECT compressed, raw_bytes, stored_bytes FROM transcript_messages ORDER BY at")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(rows[0].0, 0);
    assert_eq!(rows[0].2, rows[0].1);
    assert_eq!(rows[1].0, 1);
    assert!(rows[1].2 < rows[1].1 / 4, "{rows:?}");
    // And it comes back out byte for byte.
    assert_eq!(world.texts("sess"), ["short", long.as_str()]);
}

#[test]
fn a_raw_deflate_body_reads_and_searches_decoded_and_one_that_cannot_be_inflated_reads_as_empty() {
    use std::io::Write as _;
    let mut world = Archive::new();
    let long = "where is the zorblatt configured, asked again and again. ".repeat(30);
    let other = "a second conversation about the gizmo, at some length. ".repeat(30);
    let messages = [world.user(1, &long), world.user(2, "a short placeholder")];
    world.archive("new.jsonl", "sess", &messages);
    let messages = [world.user(3, "another short placeholder")];
    world.archive("old.jsonl", "older", &messages);
    // `older`'s body as another writer of `floti.db` stores it, in raw
    // deflate, and `sess`'s short one marked compressed when it is not.
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(other.as_bytes()).unwrap();
    let packed = encoder.finish().unwrap();
    world
        .conn()
        .execute(
            "UPDATE transcript_messages SET body = ?, compressed = 1 WHERE session_id = 'older'",
            [&packed],
        )
        .unwrap();
    world
        .conn()
        .execute(
            "UPDATE transcript_messages SET body = X'DEADBEEF00FF', compressed = 1
             WHERE session_id = 'sess' AND compressed = 0",
            [],
        )
        .unwrap();

    // Read: decoded, and the body that cannot be inflated is empty text.
    assert_eq!(world.texts("sess"), [long.as_str(), ""]);
    assert_eq!(world.texts("older"), [other.as_str()]);
    // Searched: the match is handed back decoded, as the part around it, and
    // the unreadable body fails nothing.
    assert_eq!(
        world.search("zorblatt").get("sess"),
        Some(&archive::excerpt(&long, "zorblatt"))
    );
    let placeholders = world.search("placeholder");
    assert_eq!(
        placeholders.get("older"),
        Some(&archive::excerpt(&other, "placeholder"))
    );
    assert_eq!(placeholders.get("sess").map(String::as_str), Some(""));
}

#[test]
fn a_token_planted_in_the_middle_of_a_conversation_is_found() {
    let mut world = Archive::new();
    let messages = [
        world.user(1, "opening question"),
        world.message(ArchiveRole::Assistant, 2, "the answer is zorblatt"),
        world.user(3, "closing remark"),
    ];
    world.archive("search.jsonl", "sess", &messages);
    assert_eq!(
        world.search("zorblatt").into_keys().collect::<Vec<_>>(),
        ["sess"]
    );
    assert!(world.search("zorbl")["sess"].contains("zorblatt"));
    assert!(world.search("nothinglikethis").is_empty());
}

#[test]
fn a_message_search_finds_it_through_the_history_list_and_a_prompt_search_does_not() {
    let mut world = Archive::new();
    let backend = BackendId::claude_code();
    let index = HistoryIndex::new(world.store.connection(), &backend);
    let lines: Vec<PromptLine> = ["opening question", "closing remark"]
        .iter()
        .enumerate()
        .map(|(n, text)| PromptLine {
            session_id: "sess".to_owned(),
            project: "folder".to_owned(),
            at: 1_760_000_000_000 + n as u64,
            text: (*text).to_owned(),
        })
        .collect();
    index
        .index(&IndexInput {
            record: RECORD,
            bytes: 100,
            reset: false,
            replaced: &BTreeSet::new(),
            lines: &lines,
            transcripts: &BTreeMap::new(),
            directory_exists: &|_| true,
            error: None,
        })
        .unwrap();
    let messages = [
        world.user(1, "opening question"),
        world.message(ArchiveRole::Assistant, 2, "the answer is zorblatt"),
        world.user(3, "closing remark"),
    ];
    world.archive("scoped.jsonl", "sess", &messages);
    let index = HistoryIndex::new(world.store.connection(), &backend);
    let scoped = |scope| {
        index
            .sessions(&HistoryQuery {
                search: Some("zorblatt".to_owned()),
                scope: Some(scope),
                ..HistoryQuery::default()
            })
            .unwrap()
    };
    let found = scoped(HistorySearchScope::Messages);
    assert_eq!(
        found
            .sessions
            .iter()
            .map(|s| (s.session_id.as_str(), s.matched.as_deref()))
            .collect::<Vec<_>>(),
        [("sess", Some("the answer is zorblatt"))]
    );
    assert_eq!(scoped(HistorySearchScope::Prompts).total, 0);
}

#[test]
fn a_message_search_gives_each_conversation_its_first_two_matching_lines_and_their_count() {
    let mut world = Archive::new();
    let backend = BackendId::claude_code();
    let lines = vec![PromptLine {
        session_id: "sess".to_owned(),
        project: "folder".to_owned(),
        at: 1_760_000_000_000,
        text: "opening question".to_owned(),
    }];
    HistoryIndex::new(world.store.connection(), &backend)
        .index(&IndexInput {
            record: RECORD,
            bytes: 100,
            reset: false,
            replaced: &BTreeSet::new(),
            lines: &lines,
            transcripts: &BTreeMap::new(),
            directory_exists: &|_| true,
            error: None,
        })
        .unwrap();
    let messages = [
        world.message(ArchiveRole::Assistant, 30, "third zorblatt"),
        world.user(10, "first zorblatt"),
        world.user(15, "nothing here"),
        world.message(ArchiveRole::Assistant, 20, "second zorblatt"),
    ];
    world.archive("lines.jsonl", "sess", &messages);
    let page = HistoryIndex::new(world.store.connection(), &backend)
        .sessions(&HistoryQuery {
            search: Some("zorblatt".to_owned()),
            scope: Some(HistorySearchScope::Messages),
            ..HistoryQuery::default()
        })
        .unwrap();
    let found = &page.sessions[0];
    let matched: Vec<(&str, u64)> = found
        .matches
        .as_ref()
        .unwrap()
        .iter()
        .map(|line| (line.text.as_str(), line.at))
        .collect();
    assert_eq!(
        matched,
        [
            ("first zorblatt", messages[1].at),
            ("second zorblatt", messages[3].at)
        ]
    );
    assert_eq!(found.match_count, Some(3));
    let ids: Vec<&str> = messages.iter().map(|m| m.uuid.as_str()).collect();
    assert_eq!(found.matches.as_ref().unwrap()[0].uuid, ids[1]);
}

#[test]
fn input_fts5_would_refuse_matches_nothing_rather_than_failing() {
    let mut world = Archive::new();
    let messages = [world.user(1, "ordinary words")];
    world.archive("odd.jsonl", "sess", &messages);
    for term in ["\"", "(", "NEAR(", "a AND", "*", "^"] {
        archive::search_archive(world.conn(), term, 2000).expect("no failure");
    }
}

#[test]
fn each_session_carries_its_agents_last_reply_without_tool_calls() {
    let mut world = Archive::new();
    let backend = BackendId::claude_code();
    let lines: Vec<PromptLine> = ["said", "silent"]
        .iter()
        .map(|id| PromptLine {
            session_id: (*id).to_owned(),
            project: "folder".to_owned(),
            at: 1_760_000_000_000,
            text: "a question".to_owned(),
        })
        .collect();
    HistoryIndex::new(world.store.connection(), &backend)
        .index(&IndexInput {
            record: RECORD,
            bytes: 100,
            reset: false,
            replaced: &BTreeSet::new(),
            lines: &lines,
            transcripts: &BTreeMap::new(),
            directory_exists: &|_| true,
            error: None,
        })
        .unwrap();
    let long = "word ".repeat(100);
    // One row per part of a message, as Claude Code writes its transcript.
    let said = [
        world.user(1, "a question"),
        world.message(ArchiveRole::Assistant, 2, "[tool: Read]"),
        world.message(ArchiveRole::Assistant, 3, "The watcher holds   a\nhandle."),
        world.user(4, "and now?"),
        world.message(ArchiveRole::Assistant, 5, "[tool: Edit]"),
        world.message(ArchiveRole::Assistant, 6, "[tool: Bash]"),
        world.message(
            ArchiveRole::Assistant,
            7,
            &format!("Closing it fixes {long}"),
        ),
    ];
    world.archive("said.jsonl", "said", &said);
    let silent = [world.user(1, "a question")];
    world.archive("silent.jsonl", "silent", &silent);

    let index = HistoryIndex::new(world.store.connection(), &backend);
    let page = index.sessions(&HistoryQuery::default()).unwrap();
    let reply = |id: &str| {
        page.sessions
            .iter()
            .find(|s| s.session_id == id)
            .unwrap()
            .last_reply
            .clone()
    };
    let last = reply("said").expect("a reply that says something");
    assert!(last.starts_with("Closing it fixes word word"), "{last}");
    assert!(last.ends_with('…'));
    assert_eq!(last.chars().count(), archive::LAST_REPLY_CHARS + 1);
    assert_eq!(reply("silent"), None);
    assert_eq!(
        index
            .session("SAID")
            .unwrap()
            .unwrap()
            .last_reply
            .as_deref(),
        Some(last.as_str())
    );
    let earlier = [
        world.user(1, "a question"),
        world.message(ArchiveRole::Assistant, 2, "The watcher holds   a\nhandle."),
    ];
    world.archive("short.jsonl", "short", &earlier);
    assert_eq!(
        archive::last_reply(world.conn(), "short")
            .unwrap()
            .as_deref(),
        Some("The watcher holds a handle.")
    );
}

#[test]
fn a_last_reply_is_the_last_row_of_a_finished_turn_and_never_a_thinking_or_tool_row() {
    let mut world = Archive::new();
    // As Claude Code writes them: a row for each thinking, text and tool
    // call, so a thinking row reads like a text row.
    let finished = [
        world.user(1, "a question"),
        world.message(ArchiveRole::Assistant, 2, "Fixed the watcher."),
        world.user(3, "and the tests?"),
        world.message(ArchiveRole::Assistant, 4, "I should run the tests."),
        world.message(ArchiveRole::Assistant, 5, "[tool: Bash]"),
        world.message(
            ArchiveRole::Assistant,
            6,
            "Weighing whether the handle leaks.",
        ),
        world.message(ArchiveRole::Assistant, 7, "The tests   pass."),
    ];
    world.archive("finished.jsonl", "finished", &finished);
    assert_eq!(
        archive::last_reply(world.conn(), "finished")
            .unwrap()
            .as_deref(),
        Some("The tests pass.")
    );
    // A turn that ends in a tool call never finished: its thinking is not
    // the reply, and the turn before it has one.
    let lost = [
        world.user(1, "a question"),
        world.message(ArchiveRole::Assistant, 2, "Fixed the watcher."),
        world.user(3, "and the tests?"),
        world.message(
            ArchiveRole::Assistant,
            4,
            "The user wants the tests too; I should run them.",
        ),
        world.message(ArchiveRole::Assistant, 5, "[tool: Bash]"),
    ];
    world.archive("lost.jsonl", "lost", &lost);
    assert_eq!(
        archive::last_reply(world.conn(), "lost")
            .unwrap()
            .as_deref(),
        Some("Fixed the watcher.")
    );
    // Nor does a turn that ends in thinking: interrupted while it thought.
    let interrupted = [
        world.user(1, "a question"),
        world.message(ArchiveRole::Assistant, 2, "Fixed the watcher."),
        world.user(3, "and the tests?"),
        world.thinking(4, "The user wants the tests too."),
        world.user(5, "[Request interrupted by user]"),
    ];
    world.archive("interrupted.jsonl", "interrupted", &interrupted);
    assert_eq!(
        archive::last_reply(world.conn(), "interrupted")
            .unwrap()
            .as_deref(),
        Some("Fixed the watcher.")
    );
    // A message an earlier version stored whole gives its last paragraph.
    let whole = [
        world.user(1, "a question"),
        world.message(
            ArchiveRole::Assistant,
            2,
            "Weighing whether the handle leaks first.\n\nThe tests   pass.",
        ),
    ];
    world.archive("whole.jsonl", "whole", &whole);
    assert_eq!(
        archive::last_reply(world.conn(), "whole")
            .unwrap()
            .as_deref(),
        Some("The tests pass.")
    );
}

#[test]
fn only_the_first_sessions_of_a_page_carry_their_last_reply() {
    let mut world = Archive::new();
    let backend = BackendId::claude_code();
    let ids: Vec<String> = (0..51).map(|n| format!("s{n:02}")).collect();
    let lines: Vec<PromptLine> = ids
        .iter()
        .enumerate()
        .map(|(n, id)| PromptLine {
            session_id: id.clone(),
            project: "folder".to_owned(),
            at: 1_760_000_000_000 + n as u64,
            text: "a question".to_owned(),
        })
        .collect();
    HistoryIndex::new(world.store.connection(), &backend)
        .index(&IndexInput {
            record: RECORD,
            bytes: 100,
            reset: false,
            replaced: &BTreeSet::new(),
            lines: &lines,
            transcripts: &BTreeMap::new(),
            directory_exists: &|_| true,
            error: None,
        })
        .unwrap();
    for id in &ids {
        let messages = [
            world.user(1, "a question"),
            world.message(ArchiveRole::Assistant, 2, "Done."),
        ];
        world.archive(&format!("{id}.jsonl"), id, &messages);
    }
    let page = HistoryIndex::new(world.store.connection(), &backend)
        .sessions(&HistoryQuery::default())
        .unwrap();
    assert_eq!(page.sessions.len(), 51);
    assert!(
        page.sessions[..50]
            .iter()
            .all(|s| s.last_reply.as_deref() == Some("Done.")),
        "the first fifty carry it"
    );
    assert_eq!(page.sessions[50].session_id, "s00");
    assert_eq!(page.sessions[50].last_reply, None);
}

#[test]
fn a_matching_message_is_sent_as_the_part_around_its_match() {
    let mut world = Archive::new();
    let backend = BackendId::claude_code();
    let lines = vec![PromptLine {
        session_id: "long".to_owned(),
        project: "folder".to_owned(),
        at: 1_760_000_000_000,
        text: "opening question".to_owned(),
    }];
    HistoryIndex::new(world.store.connection(), &backend)
        .index(&IndexInput {
            record: RECORD,
            bytes: 100,
            reset: false,
            replaced: &BTreeSet::new(),
            lines: &lines,
            transcripts: &BTreeMap::new(),
            directory_exists: &|_| true,
            error: None,
        })
        .unwrap();
    let body = format!(
        "{}The Zorblatt handle leaks.{}",
        "before ".repeat(5_000),
        " after".repeat(5_000)
    );
    let messages = [world.message(ArchiveRole::Assistant, 1, &body)];
    world.archive("long.jsonl", "long", &messages);

    let found = world.search("zorblatt");
    let matched = &found["long"];
    assert!(
        matched.chars().count() <= archive::MATCH_EXCERPT_CHARS + 2,
        "{matched}"
    );
    assert!(matched.contains("The Zorblatt handle leaks."), "{matched}");
    assert!(
        matched.starts_with('…') && matched.ends_with('…'),
        "{matched}"
    );

    let page = HistoryIndex::new(world.store.connection(), &backend)
        .sessions(&HistoryQuery {
            search: Some("zorblatt".to_owned()),
            scope: Some(HistorySearchScope::Messages),
            ..HistoryQuery::default()
        })
        .unwrap();
    let session = &page.sessions[0];
    assert_eq!(session.matched.as_deref(), Some(matched.as_str()));
    let line = &session.matches.as_ref().unwrap()[0].text;
    assert_eq!(line, matched);

    // A message with its match at the very start is cut at its end only.
    assert_eq!(
        archive::excerpt(&format!("zorblatt{}", " after".repeat(100)), "zorblatt")
            .chars()
            .count(),
        archive::MATCH_EXCERPT_CHARS + 1
    );
}

#[test]
fn an_archive_that_cannot_be_searched_is_an_error_and_never_no_match() {
    let mut world = Archive::new();
    let backend = BackendId::claude_code();
    let messages = [world.user(1, "the word is zorblatt")];
    world.archive("broken.jsonl", "sess", &messages);
    world
        .conn()
        .execute_batch("DROP TABLE transcript_fts")
        .expect("drop the archive's search table");
    assert!(archive::search_archive(world.conn(), "zorblatt", 10).is_err());
    assert!(archive::matching_lines(world.conn(), "zorblatt", "sess", 2).is_err());
    let index = HistoryIndex::new(world.store.connection(), &backend);
    let answer = index.sessions(&HistoryQuery {
        search: Some("zorblatt".to_owned()),
        scope: Some(HistorySearchScope::Messages),
        ..HistoryQuery::default()
    });
    assert!(
        answer.is_err(),
        "a failed archive search is could-not-look: {answer:?}"
    );
}

#[test]
fn a_conversation_the_ceiling_dropped_is_no_longer_found() {
    let mut world = Archive::new();
    let messages = [world.user(1, &"the word is zorblatt ".repeat(20))];
    world.archive("gone.jsonl", "sess", &messages);
    assert_eq!(world.search("zorblatt").len(), 1);
    archive::evict_to_ceiling(world.conn(), 0).unwrap();
    assert!(world.search("zorblatt").is_empty());
}

#[test]
fn the_ceiling_drops_the_oldest_session_whole_and_only_as_far_as_it_has_to() {
    let mut world = Archive::new();
    world.fill();
    let each = world.bytes() as f64 / 4.0;
    let ceiling = (each * 3.5).round() as u64;
    let eviction = archive::evict_to_ceiling(world.conn(), ceiling).unwrap();
    assert_eq!(eviction.sessions, ["oldest"]);
    assert!(world.bytes() <= ceiling);
    assert_eq!(world.state("older"), Some(ArchiveSessionState::Archived));
    assert_eq!(world.state("newest"), Some(ArchiveSessionState::Archived));
}

#[test]
fn the_ceiling_never_leaves_half_a_conversation_behind() {
    let mut world = Archive::new();
    let messages: Vec<ArchiveMessage> = (0..12)
        .map(|n| world.user(n, &format!("part {n} ").repeat(40)))
        .collect();
    world.archive("many.jsonl", "wide", &messages);
    let total = world.bytes();
    archive::evict_to_ceiling(world.conn(), total / 2).unwrap();
    let kept: i64 = world
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM transcript_messages WHERE session_id = 'wide'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kept, 0);
    assert_eq!(world.state("wide"), Some(ArchiveSessionState::Evicted));
}

#[test]
fn a_session_the_ceiling_dropped_is_told_from_one_it_never_had() {
    let mut world = Archive::new();
    world.fill();
    let ceiling = (world.bytes() as f64 * 0.6).round() as u64;
    archive::evict_to_ceiling(world.conn(), ceiling).unwrap();
    assert_eq!(world.state("oldest"), Some(ArchiveSessionState::Evicted));
    assert_eq!(world.state("never-seen"), None);
    let dropped = archive::read_conversation(world.conn(), "oldest")
        .unwrap()
        .unwrap();
    assert_eq!(dropped.state, ArchiveSessionState::Evicted);
    assert!(dropped.messages.is_empty());
    assert!(dropped.message_count > 0);
    assert!(dropped.evicted_at.is_some());
    assert_eq!(dropped.stored_bytes, 0);
    assert_eq!(
        archive::read_conversation(world.conn(), "never-seen").unwrap(),
        None
    );
}

#[test]
fn what_the_ceiling_just_dropped_is_not_archived_again() {
    let mut world = Archive::new();
    let messages = [world.user(1, &"body ".repeat(80))];
    world.archive("again.jsonl", "sess", &messages);
    archive::evict_to_ceiling(world.conn(), 0).unwrap();
    // The transcript replaced, which offers every row it ever had again.
    world
        .conn()
        .execute(
            "DELETE FROM transcript_index WHERE file = 'again.jsonl'",
            [],
        )
        .unwrap();
    let again = world.archive("again.jsonl", "sess", &messages);
    assert_eq!(again.messages, 0);
    assert!(again.skipped_evicted);
    assert_eq!(world.state("sess"), Some(ArchiveSessionState::Evicted));
}

#[test]
fn the_ceiling_stops_when_nothing_is_left_to_drop() {
    let mut world = Archive::new();
    world.fill();
    let eviction = archive::evict_to_ceiling(world.conn(), 0).unwrap();
    assert_eq!(eviction.sessions.len(), 4);
    assert_eq!(eviction.stored_bytes, 0);
}

#[test]
fn the_stats_count_what_the_archive_has_and_what_it_dropped_apart() {
    let mut world = Archive::new();
    world.fill();
    let total = world.bytes();
    archive::evict_to_ceiling(world.conn(), total / 2).unwrap();
    let stats = archive::read_stats(world.conn(), 1024 * 1024 * 1024).unwrap();
    assert_eq!(stats.sessions + stats.evicted_sessions, 4);
    assert!(stats.evicted_sessions > 0);
    assert_eq!(stats.stored_bytes, world.bytes());
    assert!(stats.raw_bytes >= stats.stored_bytes);
    assert_eq!(stats.max_bytes, 1024 * 1024 * 1024);
}
