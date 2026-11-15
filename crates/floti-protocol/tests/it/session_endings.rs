//! `session:endings`, added in protocol 4.1, answers how each session named
//! ended from the host's records, with status fields only. A 4.0 client never
//! asks it, and every frame it knows is the same (`minor_4_1` holds what 4.1
//! adds).

use floti_protocol::ProtocolVersion;
use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::common::{EndedBy, SessionStatus};
use floti_protocol::channels::sessions::{SessionEnding, SessionEndingsRequest};
use floti_protocol::record::{records, records_dir};
use floti_protocol::version::negotiate;
use serde_json::{Value, json};

const V4_0: ProtocolVersion = ProtocolVersion::new(4, 0);
const V4_1: ProtocolVersion = ProtocolVersion::new(4, 1);

fn record_text(version: ProtocolVersion) -> String {
    records(&records_dir())
        .unwrap()
        .into_iter()
        .find_map(|(found, text)| (found == version).then_some(text))
        .unwrap_or_else(|| panic!("there is no record of {version}"))
}

#[test]
fn an_ending_carries_status_fields_only_and_leaves_out_what_did_not_happen() {
    let properties: Value = serde_json::from_str::<Value>(&record_text(V4_1)).unwrap()["types"]
        ["SessionEnding"]["properties"]
        .clone();
    let mut names: Vec<&str> = properties
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "durationMs",
            "endedAt",
            "endedBy",
            "exitCode",
            "id",
            "status",
            "stopped"
        ]
    );

    let lost = SessionEnding {
        id: 7,
        status: SessionStatus::Lost,
        ended_at: None,
        duration_ms: None,
        exit_code: None,
        stopped: false,
        ended_by: None,
    };
    let wire: Value = from_payload(&to_payload(&lost).unwrap()).unwrap();
    assert_eq!(
        wire,
        json!({ "id": 7, "status": "lost", "endedAt": null, "durationMs": null, "exitCode": null })
    );
    let stopped = SessionEnding {
        stopped: true,
        ended_by: Some(EndedBy::Person),
        status: SessionStatus::Exited,
        ..lost
    };
    let read: SessionEnding = from_payload(&to_payload(&stopped).unwrap()).unwrap();
    assert_eq!(read, stopped);
    let asked: SessionEndingsRequest =
        from_payload(&to_payload(&json!({ "ids": [3, 4] })).unwrap()).unwrap();
    assert_eq!(asked.ids, vec![3, 4]);
}

#[test]
fn a_4_1_host_negotiates_the_version_a_4_0_client_speaks() {
    assert_eq!(negotiate(V4_1, V4_0), Ok(V4_0));
    assert_eq!(negotiate(V4_1, V4_1), Ok(V4_1));
}
