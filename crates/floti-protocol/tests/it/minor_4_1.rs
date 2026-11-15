//! A client on 4.0 still reads what a host on 4.1 sends: protocol 4.1 adds a
//! standing notice's `cause` (what an error notice is about: an exit in
//! error, a launch that failed or an error the child reported while it ran
//! on) and `session:endings` (how sessions a client held running ended,
//! which a 4.0 client never asks), and changes nothing else. A 4.1 notice is
//! a 4.0 notice with a field the older reader ignores.

use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::sessions::{NoticeCause, NoticeKind, StandingNotice};
use floti_protocol::record::{Change, Difference, Part, differences, records, records_dir};
use floti_protocol::version::negotiate;
use floti_protocol::{PROTOCOL_VERSION, ProtocolVersion};
use serde::Deserialize;
use serde_json::Value;

const V4_0: ProtocolVersion = ProtocolVersion::new(4, 0);
const V4_1: ProtocolVersion = ProtocolVersion::new(4, 1);

fn record_text(version: ProtocolVersion) -> String {
    records(&records_dir())
        .unwrap()
        .into_iter()
        .find_map(|(found, text)| (found == version).then_some(text))
        .unwrap_or_else(|| panic!("there is no record of {version}"))
}

fn record_of(version: ProtocolVersion) -> Value {
    serde_json::from_str(&record_text(version)).unwrap()
}

#[test]
fn protocol_4_1_adds_only_a_notices_optional_cause_the_session_endings_request_and_their_types() {
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
                name: "NoticeCause".to_owned(),
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
            Difference {
                part: Part::Type,
                name: "StandingNotice".to_owned(),
                change: Change::Changed,
            },
        ]
    );
    assert_eq!(
        record_of(V4_1)["channels"]["session:endings"]["kind"],
        "request"
    );
    let (before, after) = (record_of(V4_0), record_of(V4_1));
    let mut notice = after["types"]["StandingNotice"].clone();
    notice["properties"]
        .as_object_mut()
        .unwrap()
        .remove("cause")
        .expect("4.1 adds StandingNotice.cause");
    assert_eq!(
        notice, before["types"]["StandingNotice"],
        "StandingNotice changes only by its optional cause, so a 4.0 notice is a whole 4.1 notice"
    );
}

/// A notice as a 4.0 client reads it: without `cause`, and not strictly.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StandingNoticeV40 {
    id: String,
    kind: NoticeKind,
    text: String,
}

#[test]
fn a_4_0_client_reads_a_4_1_notice_with_a_cause_and_a_4_1_client_reads_a_4_0_one_without() {
    assert_eq!(negotiate(V4_1, V4_0), Ok(V4_0));
    let notice = StandingNotice {
        id: "notice-4".into(),
        kind: NoticeKind::Error,
        parent_id: 12,
        parent_name: "lead".into(),
        child_id: Some(13),
        child_name: Some("reviewer".into()),
        text: "reviewer reported an error: the build broke.".into(),
        since: "2026-11-14T10:00:00Z".into(),
        limit: None,
        allow_more: None,
        cause: Some(NoticeCause::Report),
    };
    let payload = to_payload(&notice).unwrap();
    let old: StandingNoticeV40 = from_payload(&payload).expect("a 4.0 client reads it");
    assert_eq!(
        (old.id.as_str(), old.kind, old.text.as_str()),
        ("notice-4", NoticeKind::Error, notice.text.as_str())
    );

    let mut wire = serde_json::to_value(&notice).unwrap();
    wire.as_object_mut().unwrap().remove("cause");
    let from_old_host: StandingNotice = from_payload(&to_payload(&wire).unwrap()).unwrap();
    assert_eq!(from_old_host.cause, None);
}
