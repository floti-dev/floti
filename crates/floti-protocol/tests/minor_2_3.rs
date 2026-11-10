//! A client on 2.2 still reads what a host on 2.3 sends, and that host still
//! reads what the client sends: protocol 2.3 adds a session record's
//! `agent`, beside the `role` it retires, the `agents:*` channels, and what
//! starting a session as an agent needs (a launch's and a resume's `agent`, a
//! launch's `chosen`, History's and the restore offer's agent, a restore's
//! `withoutAgent`), and changes nothing else. A 2.2 frame is a 2.3 frame with
//! every addition left out and a 2.3 frame is a 2.2 frame with fields the
//! older reader ignores. `role` keeps its shape and is written with
//! `agent`'s value, and a record that names only `role` is read with that as
//! its agent.

use std::collections::BTreeSet;

use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::common::SessionRecord;
use floti_protocol::channels::sessions::{
    LaunchChoices, LaunchSessionRequest, ResumeSessionRequest, SessionToRestore,
};
use floti_protocol::record::{Change, Difference, Part, differences, records, records_dir};
use floti_protocol::version::negotiate;
use floti_protocol::{PROTOCOL_VERSION, ProtocolVersion};
use serde::Deserialize;
use serde_json::{Value, json};

const V2_2: ProtocolVersion = ProtocolVersion::new(2, 2);
const V2_3: ProtocolVersion = ProtocolVersion::new(2, 3);

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

/// The types `schema` names, by `$ref`, anywhere in it.
fn refs(schema: &Value) -> Vec<String> {
    match schema {
        Value::Object(map) => map
            .iter()
            .flat_map(|(key, value)| match (key.as_str(), value) {
                ("$ref", Value::String(target)) => target
                    .strip_prefix("#/$defs/")
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
                _ => refs(value),
            })
            .collect(),
        Value::Array(items) => items.iter().flat_map(refs).collect(),
        _ => Vec::new(),
    }
}

/// The channels 2.3 adds, for a host's agents.
const AGENTS_CHANNELS: &[&str] = &[
    "agents:addShipped",
    "agents:changed",
    "agents:delete",
    "agents:list",
    "agents:read",
    "agents:restore",
    "agents:write",
];

/// Each type 2.3 changes, with the optional properties it adds: nothing else
/// of it changes.
const CHANGED_TYPES: &[(&str, &[&str])] = &[
    ("HistorySession", &["agent"]),
    ("LaunchSessionRequest", &["agent", "chosen"]),
    ("RestorableSession", &["agent", "agentProblem"]),
    ("ResumeSessionRequest", &["agent"]),
    ("SessionRecord", &["agent"]),
    ("SessionToRestore", &["withoutAgent"]),
];

/// The channels whose payloads carry what 2.3 adds outside the agents
/// channels.
const LAUNCH_CHANNELS: &[&str] = &["session:launch"];

/// Every type `channels` carry, directly or inside another.
fn carried_by(record: &Value, channels: &[&str]) -> BTreeSet<String> {
    let mut carried: BTreeSet<String> = BTreeSet::new();
    let mut reading: Vec<Value> = channels
        .iter()
        .map(|name| record["channels"][name].clone())
        .collect();
    while let Some(schema) = reading.pop() {
        for name in refs(&schema) {
            if carried.insert(name.clone()) {
                reading.push(record["types"][&name].clone());
            }
        }
    }
    carried
}

#[test]
fn protocol_2_3_adds_only_optional_fields_to_six_types_and_adds_the_agents_channels() {
    assert_eq!(record_of(V2_3)["version"], "2.3");
    let found = differences(&record_text(V2_2), &record_text(V2_3)).unwrap();
    let changed: Vec<&Difference> = found
        .iter()
        .filter(|difference| difference.change != Change::Added)
        .collect();
    let expected: Vec<Difference> = CHANGED_TYPES
        .iter()
        .map(|(name, _)| Difference {
            part: Part::Type,
            name: (*name).to_owned(),
            change: Change::Changed,
        })
        .collect();
    assert_eq!(
        changed,
        expected.iter().collect::<Vec<_>>(),
        "only these types change, and nothing is removed"
    );
    let added_channels: Vec<&str> = found
        .iter()
        .filter(|difference| difference.part == Part::Channel)
        .map(|difference| difference.name.as_str())
        .collect();
    assert_eq!(added_channels, AGENTS_CHANNELS);

    // Every type added is one the agents channels or a launch carries,
    // directly or inside another.
    let record = record_of(V2_3);
    let mut carried = carried_by(&record, AGENTS_CHANNELS);
    carried.extend(carried_by(&record, LAUNCH_CHANNELS));
    for added in found
        .iter()
        .filter(|difference| difference.part == Part::Type && difference.change == Change::Added)
    {
        assert!(
            carried.contains(&added.name),
            "2.3 adds the type {}, which neither the agents channels nor a launch carries",
            added.name
        );
    }

    // Each changed type only gains optional properties: taking them away
    // leaves the 2.2 type exactly.
    let (before, after) = (record_of(V2_2), record_of(V2_3));
    for (name, added) in CHANGED_TYPES {
        let (before, mut after) = (
            before["types"][*name].clone(),
            after["types"][*name].clone(),
        );
        for property in *added {
            after["properties"]
                .as_object_mut()
                .unwrap()
                .remove(*property)
                .unwrap_or_else(|| panic!("2.3 adds {name}.{property}"));
        }
        assert_eq!(
            after["required"], before["required"],
            "{name}'s additions are optional, so a 2.2 frame is still a whole 2.3 frame"
        );
        assert_eq!(after, before, "{name} changes only by {added:?}");
    }
    // The retired role keeps its shape, and agent has the same one.
    assert_eq!(
        record_of(V2_3)["types"]["SessionRecord"]["properties"]["agent"],
        record_of(V2_2)["types"]["SessionRecord"]["properties"]["role"]
    );
}

/// A record as a 2.2 host sends it: the agent a child was started as is
/// `role`, and there is no `agent`.
fn record_2_2(role: Option<&str>) -> Value {
    json!({
        "id": 12,
        "name": "reviewer",
        "label": null,
        "cwd": "C:\\work\\a repo",
        "branch": "main",
        "projectPath": null,
        "profileId": null,
        "argv": ["claude"],
        "conversationId": null,
        "backend": "claude-code",
        "status": "running",
        "startedAt": "2026-11-09T10:00:00.000Z",
        "endedAt": null,
        "durationMs": null,
        "exitCode": null,
        "parentId": 11,
        "parentName": "lead",
        "role": role,
    })
}

fn read(value: &Value) -> SessionRecord {
    from_payload(&to_payload(value).unwrap()).unwrap()
}

#[test]
fn a_record_that_names_only_role_is_read_with_it_as_its_agent() {
    let record = read(&record_2_2(Some("reviewer")));
    assert_eq!(record.agent.as_deref(), Some("reviewer"));
    assert_eq!(record.role.as_deref(), Some("reviewer"));

    let none = read(&record_2_2(None));
    assert_eq!((none.agent, none.role), (None, None));
}

#[test]
fn a_2_3_record_says_agent_and_role_with_one_value_whichever_was_set() {
    let base = read(&record_2_2(None));
    for (agent, role) in [
        (Some("worker"), None),
        (Some("worker"), Some("worker")),
        (Some("worker"), Some("stale")),
    ] {
        let record = SessionRecord {
            agent: agent.map(str::to_owned),
            role: role.map(str::to_owned),
            ..base.clone()
        };
        let said = serde_json::to_value(&record).unwrap();
        assert_eq!(said["agent"], "worker", "{agent:?} {role:?}");
        assert_eq!(said["role"], "worker", "{agent:?} {role:?}");
    }
    // A session started as no agent says null for both, as 2.2 said null
    // for role.
    let said = serde_json::to_value(&base).unwrap();
    assert_eq!(
        (&said["agent"], &said["role"]),
        (&Value::Null, &Value::Null)
    );
}

/// What a 2.2 client has of a session record: its fields up to 2.2, and no
/// `agent`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionRecordV22 {
    id: u64,
    name: String,
    parent_name: Option<String>,
    role: Option<String>,
}

#[test]
fn a_2_2_client_reads_a_2_3_childs_role_as_before() {
    let child = SessionRecord {
        agent: Some("reviewer".to_owned()),
        role: None,
        ..read(&record_2_2(None))
    };
    let read: SessionRecordV22 =
        from_payload(&to_payload(&child).unwrap()).expect("a 2.2 client reads a 2.3 record");
    assert_eq!(read.id, 12);
    assert_eq!(read.name, "reviewer");
    assert_eq!(read.parent_name.as_deref(), Some("lead"));
    assert_eq!(read.role.as_deref(), Some("reviewer"));
}

#[test]
fn a_record_written_and_read_again_keeps_its_agent() {
    let child = SessionRecord {
        agent: Some("lead".to_owned()),
        ..read(&record_2_2(None))
    };
    let again: SessionRecord = from_payload(&to_payload(&child).unwrap()).unwrap();
    assert_eq!(again.agent.as_deref(), Some("lead"));
    assert_eq!(again.role.as_deref(), Some("lead"));
}

#[test]
fn a_2_2_clients_launch_resume_and_restore_read_as_starting_no_agent() {
    let launch: LaunchSessionRequest = from_payload(
        &to_payload(&json!({
            "cwd": "C:\\work\\api",
            "projectPath": null,
            "name": null,
            "profileId": 4,
            "permissionMode": "plan",
            "resume": null,
            "backend": null,
            "cols": 80,
            "rows": 24,
        }))
        .unwrap(),
    )
    .expect("a 2.3 host reads a 2.2 launch");
    assert_eq!((launch.agent, launch.chosen), (None, None));

    let resume: ResumeSessionRequest =
        from_payload(&to_payload(&json!({ "sessionId": "0b9c", "cols": 80, "rows": 24 })).unwrap())
            .expect("a 2.3 host reads a 2.2 resume");
    assert_eq!(resume.agent, None);

    let restore: SessionToRestore =
        from_payload(&to_payload(&json!({ "id": 9, "cols": 80, "rows": 24 })).unwrap())
            .expect("a 2.3 host reads a 2.2 restore");
    assert!(!restore.without_agent);
}

#[test]
fn a_2_3_launch_without_its_additions_is_the_frame_a_2_2_client_sends() {
    let plain = LaunchSessionRequest {
        cwd: "C:\\work\\api".into(),
        project_path: None,
        name: None,
        profile_id: None,
        permission_mode: None,
        resume: None,
        backend: None,
        agent: None,
        chosen: None,
        cols: 80,
        rows: 24,
    };
    let said = serde_json::to_value(&plain).unwrap();
    assert!(
        said.get("agent").is_none() && said.get("chosen").is_none(),
        "{said}"
    );

    let as_agent = LaunchSessionRequest {
        agent: Some("research-lead".into()),
        chosen: Some(LaunchChoices {
            model: Some("opus".into()),
            effort: None,
        }),
        ..plain
    };
    let again: LaunchSessionRequest = from_payload(&to_payload(&as_agent).unwrap()).unwrap();
    assert_eq!(again, as_agent);
    let restore = SessionToRestore {
        id: 9,
        cols: 80,
        rows: 24,
        without_agent: false,
    };
    assert!(
        serde_json::to_value(&restore)
            .unwrap()
            .get("withoutAgent")
            .is_none(),
        "a restore with its agent is the frame 2.2 sends"
    );
}

#[test]
fn a_2_3_host_negotiates_the_version_a_2_2_client_speaks() {
    assert_eq!(PROTOCOL_VERSION, V2_3);
    assert_eq!(negotiate(V2_3, V2_2), Ok(V2_2));
    assert_eq!(negotiate(V2_3, V2_3), Ok(V2_3));
}
