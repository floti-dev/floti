//! The family on a world made here. What it says about real sessions, over
//! the listener, is `tests/agent_tools.rs`'s.

use std::sync::Mutex;

use floti_protocol::BackendId;
use floti_protocol::channels::sessions::SessionActivity;
use serde_json::json;

use super::*;
use crate::session::SessionId;

/// A machine with one hosted session, 5150, whose listing is ready only once
/// the test says so, and the titles set on its rows, by row.
struct World {
    ready: Mutex<bool>,
    titles: Mutex<Vec<(u64, String)>>,
}

impl World {
    fn new(ready: bool) -> Self {
        Self {
            ready: Mutex::new(ready),
            titles: Mutex::new(Vec::new()),
        }
    }
}

impl World {
    fn session() -> LiveSession {
        LiveSession {
            host_session_id: Some(7),
            backend: BackendId::claude_code(),
            pid: 5150,
            registered: true,
            cwd: Some("/work/alpha".into()),
            name: Some("alpha".into()),
            activity: Some(SessionActivity::Idle),
            waiting_for: None,
            message: None,
            status_since_ms: Some(1_000),
            version: Some("2.1.300".into()),
            entrypoint: Some("cli".into()),
            started_at_ms: Some(1_000),
            conversation_id: Some("conversation-of-alpha".into()),
        }
    }
}

impl SessionsWorld for World {
    fn overview(&self) -> Option<SessionsOverview> {
        lock(&self.ready).then(|| SessionsOverview {
            sessions: vec![Self::session()],
            read_at_ms: 2_000,
        })
    }

    fn facts_for(&self, host_session_id: u64) -> Option<HostedSessionFacts> {
        (host_session_id == 7).then(|| HostedSessionFacts {
            host_session_id,
            branch: Some("main".into()),
            profile: None,
            overlays: Vec::new(),
            started_at_ms: Some(1_000),
        })
    }

    fn measure(&self) -> PassFuture<'_> {
        Box::pin(async { Vec::new() })
    }

    fn set_title(&self, host_session_id: u64, title: &str) -> Result<String, String> {
        let title = crate::session::session_title(title)?;
        lock(&self.titles).push((host_session_id, title.clone()));
        Ok(title)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What a call answered: whether it failed, and its text.
async fn ask(
    tools: &SessionTools,
    session: Option<i64>,
    tool: &str,
    arguments: Value,
) -> (bool, String) {
    let caller = Caller {
        grant: crate::agent_tools::GrantId::for_tests(0),
        session: session.map(SessionId),
        shell: false,
    };
    let Value::Object(arguments) = arguments else {
        panic!("arguments are an object");
    };
    let result = tools.call(caller, tool, arguments).await;
    let text = result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect::<Vec<_>>()
        .join("\n");
    (result.is_error.unwrap_or(false), text)
}

#[tokio::test(flavor = "multi_thread")]
async fn s24_a_tool_called_before_the_listing_is_ready_says_to_try_again_and_works_once_it_is() {
    let tools = SessionTools::new();
    // Before the host has given it anything to read.
    for tool in [SESSIONS_LIST, SESSION_DETAIL] {
        let (failed, text) = ask(&tools, Some(7), tool, json!({})).await;
        assert!(failed, "{text}");
        assert!(
            text.contains("still starting up. Try again in a moment."),
            "{text}"
        );
    }

    // Given a world whose listing cannot be read yet.
    let world = Arc::new(World::new(false));
    tools.answer_from(Arc::clone(&world) as Arc<dyn SessionsWorld>);
    for (tool, arguments) in [
        (SESSIONS_LIST, json!({})),
        (SESSION_DETAIL, json!({})),
        (SESSION_DETAIL, json!({ "pid": 5150 })),
    ] {
        let (failed, text) = ask(&tools, Some(7), tool, arguments).await;
        assert!(failed, "{text}");
        assert!(text.contains("Try again in a moment."), "{text}");
    }

    // Once it is, the same session's tools answer.
    *lock(&world.ready) = true;
    let (failed, listing) = ask(&tools, Some(7), SESSIONS_LIST, json!({})).await;
    assert!(!failed, "{listing}");
    assert!(
        listing.starts_with(
            "1 Claude Code session running on this machine, 1 of them hosted by Helm."
        ),
        "{listing}"
    );
    assert!(
        listing.contains("#5150  \"alpha\"  (this session)"),
        "{listing}"
    );
    let (failed, detail) = ask(&tools, Some(7), SESSION_DETAIL, json!({})).await;
    assert!(!failed, "{detail}");
    assert!(
        detail.starts_with("#5150  \"alpha\"  (this session)"),
        "{detail}"
    );
    assert!(!detail.contains("conversation-of-alpha"), "{detail}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pid_that_is_not_a_whole_number_is_a_failed_call_and_one_that_is_no_number_is_the_callers()
 {
    let tools = SessionTools::new();
    tools.answer_from(Arc::new(World::new(true)));
    let (failed, text) = ask(&tools, Some(7), SESSION_DETAIL, json!({ "pid": 51.5 })).await;
    assert!(failed && text.contains("whole number"), "{text}");
    let (failed, text) = ask(&tools, Some(7), SESSION_DETAIL, json!({ "pid": -3 })).await;
    assert!(failed && text.contains("whole number"), "{text}");
    let (failed, text) = ask(&tools, Some(7), SESSION_DETAIL, json!({ "pid": "5150" })).await;
    assert!(
        !failed && text.starts_with("#5150  \"alpha\"  (this session)"),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn s20_a_token_whose_session_is_not_live_marks_no_session_and_has_no_own_detail() {
    let tools = SessionTools::new();
    tools.answer_from(Arc::new(World::new(true)));
    // A session the listing does not have, and a token not tied to one yet.
    for session in [Some(99), None] {
        let (failed, listing) = ask(&tools, session, SESSIONS_LIST, json!({})).await;
        assert!(!failed, "{listing}");
        assert!(listing.contains("#5150  \"alpha\""), "{listing}");
        assert!(!listing.contains("(this session)"), "{listing}");

        let (failed, detail) = ask(&tools, session, SESSION_DETAIL, json!({})).await;
        assert!(failed, "{detail}");
        assert_eq!(detail, NO_LIVE_SESSION);

        // Another session is still answered by its pid, unmarked.
        let (failed, other) = ask(&tools, session, SESSION_DETAIL, json!({ "pid": 5150 })).await;
        assert!(!failed, "{other}");
        assert!(other.starts_with("#5150  \"alpha\"\n"), "{other}");
    }
}

#[test]
fn the_family_is_todays_helm_sessions_at_its_route_from_the_shared_table_and_off_until_switched() {
    let tools = SessionTools::new();
    assert_eq!(tools.name(), "helm-sessions");
    assert_eq!(tools.name(), SESSIONS_FAMILY);
    assert_eq!(tools.route(), "/mcp/sessions");
    let table: Value =
        serde_json::from_str(include_str!("../session_tools.json")).expect("the table is JSON");
    assert_eq!(tools.instructions(), table["instructions"]);
    assert!(!tools.is_on());
    tools.switch(true);
    assert!(tools.is_on());
    // Listed exactly as the table says, with no annotation.
    let listed: Vec<Value> = tools
        .tools()
        .iter()
        .map(|tool| serde_json::to_value(tool).expect("a tool is JSON"))
        .collect();
    assert_eq!(Value::Array(listed), table["tools"]);
    // Titling its own session is the one thing a launch does without asking,
    // as the tool and as the command; looking asks as it always has.
    assert_eq!(tools.allowed_tools(), vec![SESSION_TITLE.to_owned()]);
    assert_eq!(
        tools.allowed_commands(),
        vec!["floti session title".to_owned()]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn session_title_titles_the_callers_own_row_and_reads_no_session_named_in_its_arguments() {
    let tools = SessionTools::new();
    let world = Arc::new(World::new(true));
    tools.answer_from(Arc::clone(&world) as Arc<dyn SessionsWorld>);

    let (failed, text) = ask(
        &tools,
        Some(7),
        SESSION_TITLE,
        // A session named in an argument the schema does not declare is not
        // read: which session is titled comes from the token alone.
        json!({ "title": "  Agents page: edit and delete agents ", "pid": 5150, "id": 8 }),
    )
    .await;
    assert!(!failed, "{text}");
    assert_eq!(
        text,
        "Titled this session: Agents page: edit and delete agents"
    );
    let (failed, text) = ask(
        &tools,
        Some(9),
        SESSION_TITLE,
        json!({ "title": "Other work" }),
    )
    .await;
    assert!(!failed, "{text}");
    assert_eq!(
        *lock(&world.titles),
        vec![
            (7, "Agents page: edit and delete agents".to_owned()),
            (9, "Other work".to_owned()),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn session_title_refuses_a_title_it_cannot_keep_and_a_token_with_no_session() {
    let tools = SessionTools::new();
    let world = Arc::new(World::new(true));
    tools.answer_from(Arc::clone(&world) as Arc<dyn SessionsWorld>);

    for (arguments, says) in [
        (json!({}), "Give the title as text"),
        (json!({ "title": 5 }), "Give the title as text"),
        (json!({ "title": "one\ntwo" }), "one line"),
        (json!({ "title": "x".repeat(61) }), "at most 60"),
    ] {
        let (failed, text) = ask(&tools, Some(7), SESSION_TITLE, arguments).await;
        assert!(failed && text.contains(says), "{text}");
    }
    let (failed, text) = ask(&tools, None, SESSION_TITLE, json!({ "title": "Mine" })).await;
    assert!(failed, "{text}");
    assert_eq!(text, NO_LIVE_SESSION);
    assert!(lock(&world.titles).is_empty());
}

#[test]
fn a_session_is_told_helm_is_still_starting_or_has_no_live_session_for_its_token() {
    assert!(STILL_STARTING.starts_with("Helm cannot answer that right now"));
    assert!(NO_LIVE_SESSION.starts_with("Helm has no live session for this token"));
}
