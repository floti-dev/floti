//! A client on 2.1 still reads what a host on 2.2 sends, and that host still
//! reads what the client sends: protocol 2.2 only adds a session's title and a
//! History session's agent title, both optional, so a 2.1 frame is a 2.2
//! frame with them left out, and a 2.2 frame is a 2.1 frame with fields the
//! older reader ignores.

use std::collections::BTreeSet;

use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::common::SessionRecord;
use floti_protocol::channels::sessions::HistorySession;
use floti_protocol::record::{records, records_dir};
use floti_protocol::version::negotiate;
use floti_protocol::{PROTOCOL_VERSION, ProtocolVersion};
use serde_json::{Value, json};

const V2_1: ProtocolVersion = ProtocolVersion::new(2, 1);
const V2_2: ProtocolVersion = ProtocolVersion::new(2, 2);

fn record_of(version: ProtocolVersion) -> Value {
    let text = records(&records_dir())
        .unwrap()
        .into_iter()
        .find_map(|(found, text)| (found == version).then_some(text))
        .unwrap_or_else(|| panic!("there is no record of {version}"));
    serde_json::from_str(&text).unwrap()
}

fn keys_of(record: &Value, name: &str, part: &str) -> BTreeSet<String> {
    match &record["types"][name][part] {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(list) => list
            .iter()
            .filter_map(|name| name.as_str().map(str::to_owned))
            .collect(),
        _ => BTreeSet::new(),
    }
}

fn channels_of(record: &Value) -> BTreeSet<String> {
    record["channels"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

#[test]
fn protocol_2_2_adds_only_the_two_optional_titles_and_no_channel() {
    let v21 = record_of(V2_1);
    let v22 = record_of(V2_2);
    assert_eq!(v22["version"], "2.2");
    assert_eq!(channels_of(&v22), channels_of(&v21));
    let added = [("SessionRecord", "title"), ("HistorySession", "agentTitle")];
    for (name, field) in added {
        let before = keys_of(&v21, name, "properties");
        let after = keys_of(&v22, name, "properties");
        let gained: BTreeSet<String> = after.difference(&before).cloned().collect();
        assert_eq!(gained, BTreeSet::from([field.to_owned()]), "{name} gained");
        assert!(before.is_subset(&after), "{name} lost a field");
        assert_eq!(
            keys_of(&v22, name, "required"),
            keys_of(&v21, name, "required"),
            "{name} requires what it did, so a 2.1 frame is still a whole 2.2 frame"
        );
    }
}

/// A record as a 2.1 host sends it: with no title.
fn record_2_1() -> Value {
    json!({
        "id": 12,
        "name": "helm-3",
        "label": null,
        "cwd": "C:\\work\\a repo",
        "branch": "main",
        "projectPath": null,
        "profileId": null,
        "argv": ["claude"],
        "conversationId": null,
        "backend": "claude-code",
        "status": "running",
        "startedAt": "2026-11-07T10:00:00.000Z",
        "endedAt": null,
        "durationMs": null,
        "exitCode": null
    })
}

#[test]
fn a_record_from_a_2_1_host_reads_as_no_title_and_a_titled_record_says_it() {
    let read: SessionRecord = from_payload(&to_payload(&record_2_1()).unwrap()).unwrap();
    assert_eq!(read.title, None);
    // Nothing is said of a title the session does not have.
    let said = serde_json::to_value(&read).unwrap();
    assert!(said.get("title").is_none(), "{said}");

    let titled = SessionRecord {
        title: Some("Agents page: edit and delete agents".into()),
        ..read
    };
    let said = serde_json::to_value(&titled).unwrap();
    assert_eq!(said["title"], "Agents page: edit and delete agents");
    // What a 2.1 client reads of it is every field it knows, as they were.
    let mut known = said.as_object().unwrap().clone();
    known.remove("title");
    let again: SessionRecord = from_payload(&to_payload(&Value::Object(known)).unwrap()).unwrap();
    assert_eq!(again.name, "helm-3");
    assert_eq!(again.title, None);
}

#[test]
fn a_history_session_from_a_2_1_host_reads_as_no_agent_title() {
    let session = json!({
        "sessionId": "0b9c",
        "backend": "claude-code",
        "project": "C:\\work\\api",
        "projectName": "api",
        "promptCount": 14,
        "firstAt": 1_761_550_000_000_i64,
        "lastAt": 1_761_560_000_000_i64,
        "firstPrompt": "/review",
        "title": "Review the auth change",
        "titleFallback": false,
        "label": null,
        "transcriptFile": null,
        "transcriptBytes": null,
        "projectExists": true
    });
    let read: HistorySession = from_payload(&to_payload(&session).unwrap()).unwrap();
    assert_eq!(read.agent_title, None);
    assert!(
        serde_json::to_value(&read)
            .unwrap()
            .get("agentTitle")
            .is_none()
    );
}

#[test]
fn a_2_2_host_negotiates_the_version_a_2_1_client_speaks() {
    assert_eq!(PROTOCOL_VERSION, V2_2);
    assert_eq!(negotiate(V2_2, V2_1), Ok(V2_1));
    assert_eq!(negotiate(V2_2, V2_2), Ok(V2_2));
}
