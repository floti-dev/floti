//! A client on 2.1 still reads what a host on 2.2 sends, and that host still
//! reads what the client sends: protocol 2.2 only adds a session's title, how
//! it ended and whether its parent kept it, a History session's agent title,
//! all optional, and `session:remove`, so a 2.1 frame is a 2.2 frame with them
//! left out, and a 2.2 frame is a 2.1 frame with fields the older reader
//! ignores.

use std::collections::BTreeSet;

use floti_protocol::ProtocolVersion;
use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::common::{EndedBy, SessionRecord};
use floti_protocol::channels::sessions::HistorySession;
use floti_protocol::record::{records, records_dir};
use floti_protocol::version::negotiate;
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
fn protocol_2_2_adds_only_optional_fields_and_the_remove_channel() {
    let v21 = record_of(V2_1);
    let v22 = record_of(V2_2);
    assert_eq!(v22["version"], "2.2");
    let before = channels_of(&v21);
    let after = channels_of(&v22);
    assert!(before.is_subset(&after), "every 2.1 channel stays");
    let gained: BTreeSet<String> = after.difference(&before).cloned().collect();
    assert_eq!(gained, BTreeSet::from(["session:remove".to_owned()]));
    let added: [(&str, &[&str]); 2] = [
        ("SessionRecord", &["title", "endedBy", "kept"]),
        ("HistorySession", &["agentTitle"]),
    ];
    for (name, fields) in added {
        let before = keys_of(&v21, name, "properties");
        let after = keys_of(&v22, name, "properties");
        let gained: BTreeSet<String> = after.difference(&before).cloned().collect();
        let expected: BTreeSet<String> = fields.iter().map(|field| (*field).to_owned()).collect();
        assert_eq!(gained, expected, "{name} gained");
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
fn a_record_from_a_2_1_host_says_nothing_of_how_it_ended_and_a_2_2_record_names_who_stopped_it() {
    let mut ended = record_2_1();
    ended["status"] = json!("exited");
    ended["exitCode"] = json!(1);
    ended["stopped"] = json!(true);
    let read: SessionRecord = from_payload(&to_payload(&ended).unwrap()).unwrap();
    assert_eq!(read.ended_by, None);
    assert!(read.stopped);
    let said = serde_json::to_value(&read).unwrap();
    assert!(said.get("endedBy").is_none(), "{said}");

    for (by, word) in [
        (EndedBy::Exited, "exited"),
        (EndedBy::Parent, "parent"),
        (EndedBy::Person, "person"),
        (EndedBy::Floti, "floti"),
    ] {
        let told = SessionRecord {
            ended_by: Some(by),
            ..read.clone()
        };
        let said = serde_json::to_value(&told).unwrap();
        assert_eq!(said["endedBy"], word);
        let back: SessionRecord = from_payload(&to_payload(&said).unwrap()).unwrap();
        assert_eq!(back.ended_by, Some(by));
    }
}

#[test]
fn a_record_from_a_2_1_host_reads_as_not_kept_and_only_a_kept_child_says_it() {
    let mut ended = record_2_1();
    ended["status"] = json!("exited");
    ended["parentId"] = json!(4);
    ended["stopped"] = json!(true);
    let read: SessionRecord = from_payload(&to_payload(&ended).unwrap()).unwrap();
    assert!(!read.kept);
    let said = serde_json::to_value(&read).unwrap();
    assert!(said.get("kept").is_none(), "{said}");

    let kept = SessionRecord { kept: true, ..read };
    let said = serde_json::to_value(&kept).unwrap();
    assert_eq!(said["kept"], true);
    let back: SessionRecord = from_payload(&to_payload(&said).unwrap()).unwrap();
    assert!(back.kept);
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
    assert_eq!(negotiate(V2_2, V2_1), Ok(V2_1));
    assert_eq!(negotiate(V2_2, V2_2), Ok(V2_2));
}
