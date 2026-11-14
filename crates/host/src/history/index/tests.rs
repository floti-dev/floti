use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use floti_protocol::BackendId;
use floti_protocol::channels::common::EndedBy;
use floti_protocol::channels::sessions::{
    HistoryPage, HistoryQuery, HistorySession, HistorySummary,
};

use super::{
    HistoryIndex, IndexInput, PromptLine, RANK_PROBE, TranscriptFile, clean_name, like_pattern,
    title_rank,
};
use crate::history::archive;
use crate::store::Store;
use crate::testkit::TestDirs;

#[test]
fn a_prompts_rank_is_read_back_from_the_title_rule() {
    assert_eq!(title_rank("fix the login page redirect"), 0);
    assert_eq!(title_rank("/spec:quick when a user goes to admin"), 0);
    assert_eq!(title_rank("/usage"), 1);
    assert_eq!(title_rank("ok"), 1);
    assert_eq!(title_rank(RANK_PROBE), 0);
    assert_eq!(title_rank("[Image #1]"), 2);
    assert_eq!(title_rank(""), 2);
}

#[test]
fn like_wildcards_typed_in_a_search_are_literal() {
    assert_eq!(like_pattern("50%"), "%50\\%%");
    assert_eq!(like_pattern("a_b\\c"), "%a\\_b\\\\c%");
}

#[test]
fn a_name_loses_the_characters_that_would_corrupt_its_row() {
    assert_eq!(clean_name("alpha beta\u{200B}gamma"), "alpha beta gamma");
    assert_eq!(clean_name("  two\t\nlines  "), "two lines");
    assert_eq!(clean_name("\u{202E}reversed"), "reversed");
}

// ---------------------------------------------------------------------------
// The index on its own
// ---------------------------------------------------------------------------

/// The record the index tests feed, as a path on any OS.
const RECORD: &str = "history.jsonl";

/// A store in a temporary data folder.
struct Indexed {
    store: Store,
    backend: BackendId,
    alpha: String,
    beta: String,
    clock: u64,
    transcripts_dir: PathBuf,
    /// Last: fields drop in order, and the folder goes only once nothing in it is open.
    _dirs: TestDirs,
}

/// One prompt to index; `project` and `at` default as core's tests do.
struct Prompt<'a> {
    session: &'a str,
    text: &'a str,
    project: Option<&'a str>,
}

fn p<'a>(session: &'a str, text: &'a str) -> Prompt<'a> {
    Prompt {
        session,
        text,
        project: None,
    }
}

fn pin<'a>(session: &'a str, text: &'a str, project: &'a str) -> Prompt<'a> {
    Prompt {
        session,
        text,
        project: Some(project),
    }
}

#[derive(Default)]
struct Pass<'a> {
    transcripts: &'a [&'a str],
    missing: &'a [&'a str],
    bytes: Option<u64>,
    reset: bool,
    error: Option<&'a str>,
}

impl Indexed {
    fn new() -> Self {
        let dirs = TestDirs::new();
        let store = Store::open(&dirs.data).expect("open the store");
        Self {
            alpha: dirs.folder("repos/alpha").display().to_string(),
            beta: dirs.folder("repos/beta").display().to_string(),
            transcripts_dir: dirs.folder("transcripts"),
            store,
            backend: BackendId::claude_code(),
            clock: 1_700_000_000_000,
            _dirs: dirs,
        }
    }

    fn index(&self) -> HistoryIndex<'_> {
        HistoryIndex::new(self.store.connection(), &self.backend)
    }

    fn transcripts(&self, ids: &[&str]) -> BTreeMap<String, TranscriptFile> {
        ids.iter()
            .map(|id| {
                (
                    id.to_lowercase(),
                    TranscriptFile {
                        file: self
                            .transcripts_dir
                            .join(format!("{id}.jsonl"))
                            .display()
                            .to_string(),
                        bytes: 1234,
                    },
                )
            })
            .collect()
    }

    fn pass(&mut self, prompts: &[Prompt<'_>], pass: Pass<'_>) -> HistorySummary {
        let lines: Vec<PromptLine> = prompts
            .iter()
            .map(|prompt| {
                self.clock += 1000;
                PromptLine {
                    session_id: prompt.session.to_owned(),
                    project: prompt.project.unwrap_or(&self.alpha).to_owned(),
                    at: self.clock,
                    text: prompt.text.to_owned(),
                }
            })
            .collect();
        let missing: BTreeSet<String> = pass.missing.iter().map(|p| p.to_lowercase()).collect();
        let transcripts = self.transcripts(pass.transcripts);
        self.index()
            .index(&IndexInput {
                record: RECORD,
                bytes: pass.bytes.unwrap_or(100),
                reset: pass.reset,
                replaced: &BTreeSet::new(),
                lines: &lines,
                transcripts: &transcripts,
                directory_exists: &|folder| !missing.contains(&folder.to_lowercase()),
                error: pass.error.map(str::to_owned),
            })
            .expect("index a pass")
    }

    fn sessions(&self, query: HistoryQuery) -> HistoryPage {
        self.index().sessions(&query).expect("read sessions")
    }

    fn all(&self) -> HistoryPage {
        self.sessions(HistoryQuery::default())
    }

    fn session(&self, id: &str) -> Option<HistorySession> {
        self.index().session(id).expect("read a session")
    }

    fn search(&self, search: &str) -> HistoryPage {
        self.sessions(HistoryQuery {
            search: Some(search.to_owned()),
            ..HistoryQuery::default()
        })
    }

    fn sql_count(&self, sql: &str) -> i64 {
        self.store
            .connection()
            .query_row(sql, [], |row| row.get(0))
            .unwrap()
    }
}

fn ids(page: &HistoryPage) -> Vec<&str> {
    page.sessions
        .iter()
        .map(|session| session.session_id.as_str())
        .collect()
}

fn can_resume(session: &HistorySession) -> bool {
    session.transcript_file.is_some() && session.project_exists
}

#[test]
fn a_record_of_prompts_becomes_sessions_ordered_by_recency() {
    let mut world = Indexed::new();
    let beta = world.beta.clone();
    world.pass(
        &[
            p("a", "first thing"),
            p("a", "second thing"),
            pin("b", "another session", &beta),
        ],
        Pass::default(),
    );
    let page = world.all();
    assert_eq!(page.total, 2);
    assert_eq!(ids(&page), ["b", "a"]);
    let a = &page.sessions[1];
    assert_eq!(a.project, world.alpha);
    assert_eq!(a.project_name, "alpha");
    assert_eq!(a.prompt_count, 2);
    assert_eq!(a.first_prompt, "first thing");
    assert_eq!(a.backend, BackendId::claude_code());
}

#[test]
fn the_cursor_is_recorded_so_the_next_pass_can_resume_from_it() {
    let mut world = Indexed::new();
    world.pass(
        &[p("a", "x")],
        Pass {
            bytes: Some(4096),
            ..Pass::default()
        },
    );
    assert_eq!(world.index().cursor(RECORD).unwrap(), 4096);
    assert_eq!(world.index().cursor("somewhere else.jsonl").unwrap(), 0);
}

#[test]
fn an_appended_pass_merges_into_the_sessions_already_indexed() {
    let mut world = Indexed::new();
    world.pass(&[p("a", "first")], Pass::default());
    world.pass(
        &[p("a", "follow up"), p("c", "new session")],
        Pass::default(),
    );
    let page = world.all();
    assert_eq!(page.sessions.len(), 2);
    let a = page.sessions.iter().find(|s| s.session_id == "a").unwrap();
    assert_eq!(a.prompt_count, 2);
    // The opening prompt is the opening prompt, not the most recent one.
    assert_eq!(a.first_prompt, "first");
    assert!(a.last_at > a.first_at);
}

#[test]
fn everything_is_discarded_when_the_record_has_been_replaced() {
    let mut world = Indexed::new();
    world.pass(&[p("a", "old world")], Pass::default());
    world.pass(
        &[p("z", "new world")],
        Pass {
            reset: true,
            ..Pass::default()
        },
    );
    assert_eq!(ids(&world.all()), ["z"]);
    assert!(world.index().prompts("a").unwrap().is_empty());
}

#[test]
fn a_session_is_resumable_only_when_its_transcript_and_its_folder_are_both_there() {
    let mut world = Indexed::new();
    let beta = world.beta.clone();
    world.pass(
        &[
            p("kept", "has a transcript"),
            p("reaped", "transcript is gone"),
            pin("homeless", "project is gone", &beta),
        ],
        Pass {
            transcripts: &["kept", "homeless"],
            missing: &[&beta],
            ..Pass::default()
        },
    );
    let kept = world.session("kept").unwrap();
    assert!(can_resume(&kept));
    assert_eq!(kept.transcript_bytes, Some(1234));
    // Reaped: history only, and its prompts still there to read.
    let reaped = world.session("reaped").unwrap();
    assert_eq!(reaped.transcript_file, None);
    assert!(!can_resume(&reaped));
    // The transcript survived and its folder did not, which a resume cannot
    // work around.
    let homeless = world.session("homeless").unwrap();
    assert!(homeless.transcript_file.is_some());
    assert!(!homeless.project_exists);
    assert!(!can_resume(&homeless));
}

#[test]
fn a_transcript_reaped_since_the_last_pass_is_noticed() {
    let mut world = Indexed::new();
    world.pass(
        &[p("a", "x")],
        Pass {
            transcripts: &["a"],
            ..Pass::default()
        },
    );
    assert!(world.session("a").unwrap().transcript_file.is_some());
    // Nothing new in the record; the transcript is simply gone.
    let summary = world.pass(&[], Pass::default());
    assert_eq!(world.session("a").unwrap().transcript_file, None);
    assert_eq!(summary.resumable, 0);
}

#[test]
fn a_transcript_matches_its_session_whatever_case_the_id_was_written_in() {
    let mut world = Indexed::new();
    world.pass(
        &[p("AbC", "x")],
        Pass {
            transcripts: &["aBc"],
            ..Pass::default()
        },
    );
    assert!(world.session("AbC").unwrap().transcript_file.is_some());
}

#[test]
fn a_folder_recorded_under_two_casings_is_one_project() {
    let mut world = Indexed::new();
    world.pass(
        &[
            pin("a", "x", "C:\\repos\\Alpha"),
            pin("b", "y", "C:\\repos\\alpha"),
        ],
        Pass::default(),
    );
    let projects = world.index().projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!((projects[0].sessions, projects[0].prompts), (2, 2));
    assert!(!projects[0].name.is_empty());
}

#[test]
fn running_a_pass_again_changes_nothing() {
    let mut world = Indexed::new();
    let first = world.pass(
        &[p("a", "x")],
        Pass {
            transcripts: &["a"],
            ..Pass::default()
        },
    );
    let again = world.pass(
        &[],
        Pass {
            transcripts: &["a"],
            bytes: Some(first.indexed_bytes),
            ..Pass::default()
        },
    );
    assert_eq!(again, first);
}

#[test]
fn a_read_error_is_reported_without_losing_what_was_indexed() {
    let mut world = Indexed::new();
    world.pass(&[p("a", "x")], Pass::default());
    let summary = world.pass(
        &[],
        Pass {
            error: Some("EBUSY"),
            ..Pass::default()
        },
    );
    assert_eq!(summary.error.as_deref(), Some("EBUSY"));
    assert_eq!(summary.sessions, 1);
}

/// Core's fixture for reading sessions.
fn searchable() -> Indexed {
    let mut world = Indexed::new();
    let beta = world.beta.clone();
    world.pass(
        &[
            p("a", "add geofencing to the map"),
            p("a", "now write the tests"),
            pin("b", "fix the geofence radius", &beta),
            p("c", "unrelated work"),
            p("d", "50% of the rows are _wrong_"),
        ],
        Pass {
            transcripts: &["a"],
            ..Pass::default()
        },
    );
    world
}

#[test]
fn a_search_filters_by_a_substring_of_any_prompt_in_the_session() {
    let world = searchable();
    let page = world.search("geofenc");
    assert_eq!(page.total, 2);
    let mut found = ids(&page);
    found.sort_unstable();
    assert_eq!(found, ["a", "b"]);
}

#[test]
fn a_search_matches_case_insensitively_and_says_which_prompt_matched() {
    let world = searchable();
    let page = world.search("WRITE THE TESTS");
    assert_eq!(page.sessions.len(), 1);
    assert_eq!(
        page.sessions[0].matched.as_deref(),
        Some("now write the tests")
    );
    assert_eq!(page.sessions[0].first_prompt, "add geofencing to the map");
}

#[test]
fn a_long_prompt_that_matched_is_sent_as_the_excerpt_around_its_match() {
    let mut world = Indexed::new();
    let long = format!(
        "{} then fix the Zorblatt handle leak {}",
        "context ".repeat(200),
        "trailing ".repeat(200)
    );
    world.pass(&[p("a", &long)], Pass::default());
    let page = world.search("zorblatt handle");
    let matched = page.sessions[0].matched.as_deref().expect("a match");
    assert_eq!(matched, archive::excerpt(&long, "zorblatt handle"));
    assert!(
        matched.chars().count() <= archive::MATCH_EXCERPT_CHARS + 2,
        "{matched}"
    );
    assert!(matched.contains("Zorblatt handle leak"), "{matched}");
    assert!(
        matched.starts_with('…') && matched.ends_with('…'),
        "{matched}"
    );
}

#[test]
fn nothing_searched_for_leaves_the_match_unset() {
    assert_eq!(searchable().all().sessions[0].matched, None);
}

#[test]
fn like_wildcards_in_the_search_box_are_literal_characters() {
    let world = searchable();
    assert_eq!(world.search("50%").total, 1);
    assert_eq!(world.search("_wrong_").total, 1);
    assert_eq!(world.search("%").total, 1);
    assert_eq!(world.search("r_ws").total, 0);
}

#[test]
fn a_project_filter_ignores_case() {
    let world = searchable();
    let by = |project: String| {
        world
            .sessions(HistoryQuery {
                project: Some(project),
                ..HistoryQuery::default()
            })
            .total
    };
    assert_eq!(by(world.beta.clone()), 1);
    assert_eq!(by(world.beta.to_uppercase()), 1);
}

#[test]
fn a_search_matches_the_project_path_as_well_as_the_prompts() {
    let world = searchable();
    // No prompt says "beta"; only the folder session b ran in does.
    let page = world.search("beta");
    assert_eq!(page.total, 1);
    assert_eq!(page.sessions[0].session_id, "b");
    assert_eq!(world.search("BETA").total, 1);
    // The tail of the path, separator and all, in another case.
    let tail = world.beta[world.beta.len() - "repos/beta".len()..].to_uppercase();
    assert_eq!(world.search(&tail).total, 1);
}

#[test]
fn a_session_matching_by_project_and_prompt_is_counted_once() {
    let world = searchable();
    assert_eq!(world.search("geofenc").total, 2);
    let page = world.search("e");
    assert_eq!(page.total as usize, page.sessions.len());
    let unique: BTreeSet<&str> = ids(&page).into_iter().collect();
    assert_eq!(unique.len(), page.sessions.len());
}

#[test]
fn resumable_only_lists_what_can_actually_be_resumed() {
    let page = searchable().sessions(HistoryQuery {
        resumable_only: Some(true),
        ..HistoryQuery::default()
    });
    assert_eq!(page.total, 1);
    assert_eq!(page.sessions[0].session_id, "a");
}

#[test]
fn the_match_count_is_reported_apart_from_the_page() {
    let page = searchable().sessions(HistoryQuery {
        limit: Some(2),
        ..HistoryQuery::default()
    });
    assert_eq!(page.sessions.len(), 2);
    assert_eq!(page.total, 4);
    assert!(page.took_ms >= 0.0);
}

#[test]
fn a_session_is_titled_from_the_first_prompt_that_says_something() {
    let mut world = Indexed::new();
    world.pass(
        &[
            p("a", "/usage"),
            p("a", "[Image #1]"),
            p("a", "why is the status bar blank on a fresh install"),
        ],
        Pass::default(),
    );
    let session = world.session("a").unwrap();
    assert_eq!(
        session.title,
        "why is the status bar blank on a fresh install"
    );
    assert!(!session.title_fallback);
    assert_eq!(session.first_prompt, "/usage");
}

#[test]
fn a_title_falls_back_to_something_legible_when_no_prompt_says_anything() {
    let mut world = Indexed::new();
    world.pass(&[p("a", "/usage")], Pass::default());
    let a = world.session("a").unwrap();
    assert_eq!((a.title.as_str(), a.title_fallback), ("/usage", false));
    world.pass(&[p("b", "[Image #1]")], Pass::default());
    let b = world.session("b").unwrap();
    assert_eq!((b.title.as_str(), b.title_fallback), ("Image only", true));
}

#[test]
fn a_later_pass_with_a_better_prompt_retitles_the_session() {
    let mut world = Indexed::new();
    world.pass(&[p("a", "/usage")], Pass::default());
    assert_eq!(world.session("a").unwrap().title, "/usage");
    world.pass(&[p("a", "rename the profiles pane")], Pass::default());
    assert_eq!(
        world.session("a").unwrap().title,
        "rename the profiles pane"
    );
}

#[test]
fn prompts_an_older_build_left_unranked_are_ranked() {
    let mut world = Indexed::new();
    world.pass(
        &[p("a", "/usage"), p("a", "add geofencing to the map")],
        Pass::default(),
    );
    let conn = world.store.connection();
    conn.execute("UPDATE history_prompts SET title_rank = NULL", [])
        .unwrap();
    conn.execute("UPDATE history_sessions SET title_prompt = NULL", [])
        .unwrap();
    assert_eq!(world.session("a").unwrap().title, "/usage");
    // A pass with nothing new is enough.
    world.pass(&[], Pass::default());
    assert_eq!(
        world.session("a").unwrap().title,
        "add geofencing to the map"
    );
}

fn renamable() -> Indexed {
    let mut world = Indexed::new();
    let beta = world.beta.clone();
    world.pass(
        &[
            p("a", "/usage"),
            p("a", "add geofencing to the map"),
            pin("b", "unrelated work", &beta),
        ],
        Pass::default(),
    );
    world
}

#[test]
fn a_name_given_by_hand_is_what_the_list_shows() {
    let world = renamable();
    let renamed = world
        .index()
        .rename("a", Some("  Geofencing  "))
        .unwrap()
        .unwrap();
    assert_eq!(renamed.label.as_deref(), Some("Geofencing"));
    // The derived title is still there, which is what makes clearing it work.
    assert_eq!(renamed.title, "add geofencing to the map");
    let page = world.all();
    let label = |id: &str| {
        page.sessions
            .iter()
            .find(|s| s.session_id == id)
            .unwrap()
            .label
            .clone()
    };
    assert_eq!(label("a").as_deref(), Some("Geofencing"));
    assert_eq!(label("b"), None);
}

#[test]
fn a_name_survives_a_full_reindex() {
    let mut world = renamable();
    world.index().rename("a", Some("Geofencing")).unwrap();
    world.pass(
        &[p("a", "add geofencing to the map")],
        Pass {
            reset: true,
            ..Pass::default()
        },
    );
    assert_eq!(
        world.session("a").unwrap().label.as_deref(),
        Some("Geofencing")
    );
}

#[test]
fn clearing_a_name_returns_the_row_to_its_derived_title() {
    let world = renamable();
    world.index().rename("a", Some("Geofencing")).unwrap();
    let cleared = world.index().rename("a", Some("   ")).unwrap().unwrap();
    assert_eq!(cleared.label, None);
    assert_eq!(cleared.title, "add geofencing to the map");
    assert_eq!(world.sql_count("SELECT COUNT(*) FROM history_names"), 0);
}

#[test]
fn a_session_is_found_by_the_name_it_was_given() {
    let world = renamable();
    world
        .index()
        .rename("a", Some("Monarch budget spike"))
        .unwrap();
    let page = world.search("monarch");
    assert_eq!(page.total, 1);
    assert_eq!(page.sessions[0].session_id, "a");
}

impl Indexed {
    /// A row of the host's whose agent titled it, in `conversation`: its
    /// launch's, or the one it moved to (`moved`).
    fn titled_row(&self, conversation: &str, moved: bool, title: &str) {
        let column = if moved {
            "last_claude_session_id"
        } else {
            "claude_session_id"
        };
        self.store
            .connection()
            .execute(
                &format!(
                    "INSERT INTO sessions (name, cwd, argv, {column}, status, title) \
                     VALUES ('agent', '/work', '[]', ?1, 'exited', ?2)"
                ),
                [conversation, title],
            )
            .unwrap();
    }
}

#[test]
fn a_conversation_its_agent_titled_carries_the_title_beside_its_derived_one() {
    let world = renamable();
    world.titled_row("A", false, "Agents page: edit and delete agents");
    let a = world.session("a").unwrap();
    assert_eq!(
        a.agent_title.as_deref(),
        Some("Agents page: edit and delete agents")
    );
    // The first prompt's line is still the derived title, for a list that
    // has neither a name nor an agent's title.
    assert_eq!(a.title, "add geofencing to the map");
    assert_eq!(a.label, None);
    let page = world.all();
    let listed = |id: &str| {
        page.sessions
            .iter()
            .find(|s| s.session_id == id)
            .unwrap()
            .agent_title
            .clone()
    };
    assert_eq!(
        listed("a").as_deref(),
        Some("Agents page: edit and delete agents")
    );
    assert_eq!(listed("b"), None);
}

#[test]
fn the_newest_title_of_a_conversation_is_its_title_wherever_its_rows_held_it() {
    let world = renamable();
    world.titled_row("a", false, "Old work");
    world.titled_row("a", true, "New work");
    assert_eq!(
        world.session("a").unwrap().agent_title.as_deref(),
        Some("New work")
    );
}

#[test]
fn a_name_given_by_hand_is_kept_beside_the_agents_title_and_both_are_searched() {
    let world = renamable();
    world.titled_row("a", false, "Agents page: edit and delete agents");
    let named = world
        .index()
        .rename("a", Some("Geofencing"))
        .unwrap()
        .unwrap();
    // The name is what the list shows; the title is kept for when it is cleared.
    assert_eq!(named.label.as_deref(), Some("Geofencing"));
    assert_eq!(
        named.agent_title.as_deref(),
        Some("Agents page: edit and delete agents")
    );
    let page = world.search("delete agents");
    assert_eq!(ids(&page), ["a"]);
}

#[test]
fn a_name_is_keyed_on_the_id_whatever_case_it_is_given_with() {
    let world = renamable();
    world.index().rename("A", Some("Geofencing")).unwrap();
    assert_eq!(
        world.session("a").unwrap().label.as_deref(),
        Some("Geofencing")
    );
}

#[test]
fn a_name_given_by_hand_loses_the_characters_that_would_corrupt_its_row() {
    let world = renamable();
    let renamed = world
        .index()
        .rename("a", Some("alpha beta\u{200B}gamma"))
        .unwrap()
        .unwrap();
    assert_eq!(renamed.label.as_deref(), Some("alpha beta gamma"));
}

#[test]
fn renaming_a_session_the_index_does_not_have_answers_null() {
    let world = renamable();
    assert_eq!(
        world.index().rename("nope", Some("whatever")).unwrap(),
        None
    );
    assert_eq!(world.sql_count("SELECT COUNT(*) FROM history_names"), 0);
}

impl Indexed {
    /// A row of the host's in `conversation`, started by row `parent`, and
    /// its id.
    fn family_row(&self, conversation: &str, parent: Option<i64>, ended: &str) -> i64 {
        let connection = self.store.connection();
        connection
            .execute(
                "INSERT INTO sessions (name, cwd, argv, claude_session_id, status, branch, \
                 parent_session_id, stopped, stopped_by, exit_code) \
                 VALUES ('agent', '/work', '[]', ?1, ?2, 'feat/queue', ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    conversation,
                    if ended == "running" {
                        "running"
                    } else {
                        "exited"
                    },
                    parent,
                    ended == "floti",
                    (ended == "floti").then_some("floti"),
                    (ended == "failed").then_some(1),
                ],
            )
            .unwrap();
        connection.last_insert_rowid()
    }

    /// A report child row `child` sent parent row `parent`.
    fn report(&self, parent: i64, child: i64, state: &str, text: &str) {
        self.store
            .connection()
            .execute(
                "INSERT INTO agent_inbox (parent_session_id, child_session_id, child_name, \
                 event, state, detail) VALUES (?1, ?2, 'worker', 'report', ?3, ?4)",
                rusqlite::params![parent, child, state, text],
            )
            .unwrap();
    }
}

#[test]
fn a_child_names_its_parents_conversation_and_the_parent_counts_its_children() {
    let mut world = renamable();
    world.pass(&[p("c", "write the runner")], Pass::default());
    let lead = world.family_row("A", None, "running");
    let child = world.family_row("b", Some(lead), "floti");
    world.family_row("c", Some(lead), "failed");
    world.report(lead, child, "note", "started");
    world.report(lead, child, "stuck", "The runner needs the queue's lock");

    let page = world.all();
    let listed = |id: &str| page.sessions.iter().find(|s| s.session_id == id).unwrap();
    assert_eq!(listed("a").children, Some(2));
    assert_eq!(listed("a").parent, None);
    assert_eq!(listed("a").ended_by, None, "a running row has not ended");
    assert_eq!(listed("b").parent.as_deref(), Some("a"));
    assert_eq!(listed("b").children, None);
    assert_eq!(listed("b").branch.as_deref(), Some("feat/queue"));
    assert_eq!(listed("b").ended_by, Some(EndedBy::Floti));
    assert_eq!(listed("c").ended_by, Some(EndedBy::Exited));
    assert_eq!(listed("c").exit_code, Some(1));
    assert_eq!(
        listed("b").last_report,
        None,
        "only a parent's listing reads reports"
    );

    let top = world.sessions(HistoryQuery {
        top_level: Some(true),
        ..HistoryQuery::default()
    });
    assert_eq!(ids(&top), ["a"]);

    let children = world.sessions(HistoryQuery {
        parent: Some("A".to_owned()),
        ..HistoryQuery::default()
    });
    assert_eq!(ids(&children), ["c", "b"]);
    let report = children.sessions[1].last_report.as_ref().unwrap();
    assert_eq!(
        (report.state.as_str(), report.text.as_str()),
        ("stuck", "The runner needs the queue's lock")
    );
    assert_eq!(children.sessions[0].last_report, None);
}

#[test]
fn a_parents_listing_gives_the_commands_its_sessions_ran_and_no_other_does() {
    let world = renamable();
    let lead = world.family_row("A", None, "running");
    let other = world.family_row("z", None, "running");
    let connection = world.store.connection();
    let command = |parent: i64, line: &str, started: &str, status: &str, code: Option<i64>| {
        connection
            .execute(
                "INSERT INTO sessions (name, cwd, argv, status, parent_session_id, child_name, \
                 command, started_at, exit_code) \
                 VALUES ('cmd', '/work', '[]', ?1, ?2, 'cmd', ?3, ?4, ?5)",
                rusqlite::params![status, parent, line, started, code],
            )
            .unwrap();
    };
    command(
        lead,
        "pnpm test",
        "2026-11-07T09:05:00.000Z",
        "exited",
        Some(1),
    );
    command(
        lead,
        "pnpm lint",
        "2026-11-07T09:00:00.000Z",
        "exited",
        Some(0),
    );
    command(
        lead,
        "pnpm dev",
        "2026-11-07T09:10:00.000Z",
        "running",
        None,
    );
    command(
        other,
        "cargo check",
        "2026-11-07T09:00:00.000Z",
        "exited",
        Some(0),
    );

    let page = world.sessions(HistoryQuery {
        parent: Some("a".to_owned()),
        ..HistoryQuery::default()
    });
    let commands = page
        .commands
        .expect("a parent's listing reads its commands");
    assert_eq!(
        commands
            .iter()
            .map(|c| (c.command.as_str(), c.running, c.ended_by, c.exit_code))
            .collect::<Vec<_>>(),
        [
            ("pnpm lint", false, Some(EndedBy::Exited), Some(0)),
            ("pnpm test", false, Some(EndedBy::Exited), Some(1)),
            ("pnpm dev", true, None, None),
        ]
    );
    assert_eq!(commands[0].started_at, 1_794_042_000_000);
    assert_eq!(
        world.all().commands,
        None,
        "only a parent's listing reads commands"
    );
}

#[test]
fn after_a_clear_a_child_and_its_commands_are_listed_under_the_conversation_they_were_started_in() {
    let mut world = renamable();
    world.pass(
        &[p("c", "write the runner"), p("d", "write the docs")],
        Pass::default(),
    );
    let connection = world.store.connection();
    // The lead started in conversation A and a /clear moved it to B.
    connection
        .execute(
            "INSERT INTO sessions (name, cwd, argv, claude_session_id, last_claude_session_id, \
             status) VALUES ('lead', '/work', '[]', 'A', 'B', 'running')",
            [],
        )
        .unwrap();
    let lead = connection.last_insert_rowid();
    let started_in = |conversation: &str, child: Option<&str>, command: Option<&str>| {
        connection
            .execute(
                "INSERT INTO sessions (name, cwd, argv, claude_session_id, status, \
                 parent_session_id, parent_conversation_id, child_name, command) \
                 VALUES ('child', '/work', '[]', ?1, 'exited', ?2, ?3, 'child', ?4)",
                rusqlite::params![child, lead, conversation, command],
            )
            .unwrap();
    };
    started_in("A", Some("c"), None);
    started_in("B", Some("d"), None);
    started_in("A", None, Some("pnpm lint"));
    started_in("B", None, Some("pnpm test"));

    let listed = |parent: &str| {
        let page = world.sessions(HistoryQuery {
            parent: Some(parent.to_owned()),
            ..HistoryQuery::default()
        });
        let commands: Vec<String> = page
            .commands
            .clone()
            .unwrap()
            .into_iter()
            .map(|c| c.command)
            .collect();
        let children: Vec<String> = page.sessions.iter().map(|s| s.session_id.clone()).collect();
        (children, commands)
    };
    assert_eq!(
        listed("a"),
        (vec!["c".to_owned()], vec!["pnpm lint".to_owned()])
    );
    assert_eq!(
        listed("b"),
        (vec!["d".to_owned()], vec!["pnpm test".to_owned()])
    );
}

#[test]
fn a_parents_commands_are_its_newest_as_many_as_the_page_lists() {
    let world = renamable();
    let lead = world.family_row("A", None, "running");
    let connection = world.store.connection();
    for (line, started) in [
        ("pnpm lint", "2026-11-07T09:00:00.000Z"),
        ("pnpm test", "2026-11-07T09:05:00.000Z"),
        ("pnpm dev", "2026-11-07T09:10:00.000Z"),
    ] {
        connection
            .execute(
                "INSERT INTO sessions (name, cwd, argv, status, parent_session_id, child_name, \
                 command, started_at) VALUES ('cmd', '/work', '[]', 'exited', ?1, 'cmd', ?2, ?3)",
                rusqlite::params![lead, line, started],
            )
            .unwrap();
    }
    let page = world.sessions(HistoryQuery {
        parent: Some("a".to_owned()),
        limit: Some(2),
        ..HistoryQuery::default()
    });
    assert_eq!(
        page.commands
            .unwrap()
            .iter()
            .map(|c| c.command.as_str())
            .collect::<Vec<_>>(),
        ["pnpm test", "pnpm dev"]
    );
}

#[test]
fn a_sessions_prompts_come_in_submission_order() {
    let mut world = Indexed::new();
    world.pass(
        &[
            p("a", "one"),
            p("b", "other session"),
            p("a", "two"),
            p("a", "three"),
        ],
        Pass::default(),
    );
    let texts: Vec<String> = world
        .index()
        .prompts("a")
        .unwrap()
        .into_iter()
        .map(|prompt| prompt.text)
        .collect();
    assert_eq!(texts, ["one", "two", "three"]);
    assert!(world.index().prompts("nope").unwrap().is_empty());
}

#[test]
fn the_summary_counts_what_the_launcher_puts_on_screen() {
    let mut world = Indexed::new();
    let beta = world.beta.clone();
    world.pass(
        &[p("a", "x"), p("a", "y"), pin("b", "z", &beta)],
        Pass {
            transcripts: &["a"],
            bytes: Some(900),
            ..Pass::default()
        },
    );
    let summary = world.index().summary(RECORD).unwrap();
    assert_eq!(
        (
            summary.sessions,
            summary.prompts,
            summary.projects,
            summary.resumable
        ),
        (2, 3, 2, 1)
    );
    assert_eq!(summary.history_file, RECORD);
    assert_eq!(summary.indexed_bytes, 900);
}

#[test]
fn the_summary_is_empty_rather_than_broken_before_anything_is_indexed() {
    let world = Indexed::new();
    let summary = world.index().summary(RECORD).unwrap();
    assert_eq!(
        (
            summary.sessions,
            summary.prompts,
            summary.resumable,
            summary.latest_at,
            summary.indexed_bytes
        ),
        (0, 0, 0, None, 0)
    );
}

/// The bound a keystroke has to beat. What this catches is a search that
/// stopped using the index or started doing a query per row, not a busy
/// machine: each term's best of eight runs is held to the bound, so a test
/// run sharing the processor with every other test does not fail it.
#[test]
fn a_search_over_thousands_of_prompts_answers_within_a_keystroke() {
    let mut world = Indexed::new();
    let (alpha, beta) = (world.alpha.clone(), world.beta.clone());
    let subjects = [
        "geofencing",
        "the parser",
        "release notes",
        "a flaky test",
        "the schema",
    ];
    let texts: Vec<String> = (0..3200)
        .map(|n| format!("prompt {n} about {}", subjects[n % subjects.len()]))
        .collect();
    let sessions: Vec<String> = (0..800).map(|n| format!("s{n}")).collect();
    let prompts: Vec<Prompt<'_>> = texts
        .iter()
        .enumerate()
        .map(|(n, text)| {
            let project = if n % 2 == 0 { &alpha } else { &beta };
            pin(&sessions[n % sessions.len()], text, project)
        })
        .collect();
    let summary = world.pass(&prompts, Pass::default());
    assert_eq!((summary.sessions, summary.prompts), (800, 3200));
    for term in [
        "geofenc",
        "parser",
        "prompt 31",
        "nothing matches this",
        "beta",
    ] {
        let best = (0..8)
            .map(|_| world.search(term).took_ms)
            .fold(f64::INFINITY, f64::min);
        assert!(best < 100.0, "a search for {term:?} took {best} ms at best");
    }
}
