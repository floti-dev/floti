//! A client on 4.0 still talks to a host on 4.1: protocol 4.1 adds
//! `session:endings`, which answers how each session named ended from the
//! host's records, and changes nothing else. A 4.0 client never asks it, and
//! every frame it knows is the same.

use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::common::{EndedBy, SessionStatus};
use floti_protocol::channels::sessions::{SessionEnding, SessionEndingsRequest};
use floti_protocol::record::{Change, Difference, Part, differences, records, records_dir};
use floti_protocol::version::negotiate;
use floti_protocol::{PROTOCOL_VERSION, ProtocolVersion};
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
fn protocol_4_1_adds_only_the_session_endings_request_and_its_two_types() {
    assert!(PROTOCOL_VERSION >= V4_1);
    let found = differences(&record_text(V4_0), &record_text(V4_1)).unwrap();
    assert_eq!(
        found,
        vec![
            Difference {
                part: Part::Channel,
                name: "session:endings".to_owned(),
                change: Change::Added,
            },
            Difference {
                part: Part::Type,
                name: "SessionEnding".to_owned(),
                change: Change::Added,
            },
            Difference {
                part: Part::Type,
                name: "SessionEndingsRequest".to_owned(),
                change: Change::Added,
            },
        ]
    );
    let after: Value = serde_json::from_str(&record_text(V4_1)).unwrap();
    assert_eq!(after["channels"]["session:endings"]["kind"], "request");
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
