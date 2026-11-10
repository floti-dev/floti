//! The agents channels: a host's agents listed, read, written, deleted,
//! restored and added from the shipped ones, and `agents:changed`. Each is
//! declared with its kind and no capability, since any client may show and
//! edit agents, and each payload survives a CBOR frame.

use std::collections::BTreeMap;
use std::fmt::Debug;

use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::agents::{
    AddShippedAgent, AgentChange, AgentConflict, AgentFields, AgentFile, AgentName, AgentProblem,
    AgentSummary, AgentUnusedLine, AgentWrittenValue, AgentsChange, AgentsListing, BadAgentFile,
    DeleteAgent, DeleteAgentRequest, ListAgents, ReadAgent, RestoreAgent, ShippedAgent, WriteAgent,
    WriteAgentRequest,
};
use floti_protocol::channels::common::PermissionMode;
use floti_protocol::channels::sessions::EffortLevel;
use floti_protocol::message::{Event, Request, Response};
use floti_protocol::{
    Catalogue, ChannelKind, FrameDecoder, Message, Payload, RequestChannel, encode_frame,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

fn over_the_wire<T>(value: &T, wrap: impl FnOnce(Payload) -> Message) -> T
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let mut decoder = FrameDecoder::new();
    decoder.push(&encode_frame(&wrap(to_payload(value).unwrap())).unwrap());
    let payload = match decoder.next_message().unwrap().unwrap() {
        Message::Request(Request { payload, .. })
        | Message::Response(Response { payload, .. })
        | Message::Event(Event { payload, .. }) => payload,
        other => panic!("unexpected {other:?}"),
    };
    let decoded: T = from_payload(&payload).unwrap();
    assert_eq!(&decoded, value);
    decoded
}

fn exchange<C: RequestChannel>(request: C::Request, response: C::Response)
where
    C::Request: PartialEq + Debug,
    C::Response: PartialEq + Debug,
{
    let catalogue = Catalogue::protocol();
    let wire = Request {
        id: 3,
        channel: C::NAME.into(),
        payload: to_payload(&request).unwrap(),
    };
    let decl = catalogue
        .check_request(&wire)
        .unwrap_or_else(|reply| panic!("{} refused its own request: {reply:?}", C::NAME));
    assert_eq!(decl.name(), C::NAME);
    over_the_wire(&request, |payload| {
        Message::Request(Request {
            id: 3,
            channel: C::NAME.into(),
            payload,
        })
    });
    over_the_wire(&response, |payload| {
        Message::Response(Response { id: 3, payload })
    });
}

fn problem(field: Option<&str>, line: Option<u32>, message: &str) -> AgentProblem {
    AgentProblem {
        field: field.map(str::to_owned),
        line,
        message: message.to_owned(),
    }
}

fn reviewer_file() -> AgentFile {
    AgentFile {
        name: "reviewer".into(),
        file: "reviewer.md".into(),
        in_folder: true,
        text: "---\ndescription: Reads a change\nclass: light\n---\nRead it.\n".into(),
        hash: "ab12".into(),
        values: vec![
            AgentWrittenValue {
                field: "description".into(),
                line: 2,
                value: "Reads a change".into(),
            },
            AgentWrittenValue {
                field: "class".into(),
                line: 3,
                value: "light".into(),
            },
        ],
        fields: Some(AgentFields {
            description: "Reads a change".into(),
            profile: None,
            backends: vec!["claude-code".into()],
            models: BTreeMap::from([("claude-code".into(), "opus".into())]),
            efforts: BTreeMap::from([("claude-code".into(), EffortLevel::High)]),
            permission: Some(PermissionMode::Plan),
            worktree: false,
            can_start: false,
            allowed_tools: vec!["Read".into()],
            disallowed_tools: vec![],
            command: None,
            submit: None,
            stop: None,
        }),
        body: "Read it.".into(),
        problems: vec![],
        unused_lines: vec![AgentUnusedLine {
            field: "class".into(),
            line: 3,
        }],
    }
}

#[test]
fn every_agents_channel_is_declared_with_its_kind_and_no_capability() {
    let catalogue = Catalogue::protocol();
    for (name, kind) in [
        ("agents:list", ChannelKind::Request),
        ("agents:read", ChannelKind::Request),
        ("agents:write", ChannelKind::Request),
        ("agents:delete", ChannelKind::Request),
        ("agents:restore", ChannelKind::Request),
        ("agents:addShipped", ChannelKind::Request),
        ("agents:changed", ChannelKind::Event { snapshot: false }),
    ] {
        let decl = catalogue
            .get(name)
            .unwrap_or_else(|| panic!("{name} is not declared"));
        assert_eq!(decl.kind(), kind, "{name}");
        assert_eq!(decl.capability(), None, "{name}");
    }
}

#[test]
fn every_agents_payload_survives_a_frame() {
    exchange::<ListAgents>(
        (),
        AgentsListing {
            folder: "C:\\Users\\someone\\.config\\floti\\agents".into(),
            read_only: None,
            agents: vec![AgentSummary {
                name: "default".into(),
                description: "Any installed backend".into(),
                file: None,
                hash: None,
                backends: vec!["claude-code".into()],
                worktree: false,
                can_start: false,
                unused_lines: vec![],
                fields: AgentFields {
                    description: "Any installed backend".into(),
                    profile: None,
                    backends: vec![],
                    models: BTreeMap::from([("claude-code".into(), "opus".into())]),
                    efforts: BTreeMap::new(),
                    permission: Some(PermissionMode::Plan),
                    worktree: false,
                    can_start: false,
                    allowed_tools: vec!["Read".into()],
                    disallowed_tools: vec![],
                    command: None,
                    submit: None,
                    stop: None,
                },
                cannot_run: None,
            }],
            bad: vec![BadAgentFile {
                name: "broken".into(),
                file: "broken.md".into(),
                hash: Some("cd34".into()),
                problems: vec![problem(
                    Some("worktree"),
                    Some(4),
                    "worktree must be yes, no, true or false, not maybe",
                )],
            }],
            shipped: vec![ShippedAgent {
                name: "lead".into(),
                description: "Leads the work".into(),
            }],
        },
    );
    exchange::<ReadAgent>(
        AgentName {
            name: "reviewer".into(),
        },
        reviewer_file(),
    );
    exchange::<WriteAgent>(
        WriteAgentRequest {
            name: "checker".into(),
            text: "---\ndescription: x\n---\n".into(),
            expected_hash: Some("ab12".into()),
            rename_from: Some("reviewer".into()),
        },
        AgentChange {
            changed: false,
            file: Some(reviewer_file()),
            refusal: Some("The file changed since it was read.".into()),
            problems: vec![problem(None, Some(1), "it has no front matter")],
            conflict: Some(AgentConflict {
                on_disk_hash: Some("ef56".into()),
                on_disk_text: Some("---\n---\n".into()),
            }),
        },
    );
    exchange::<DeleteAgent>(
        DeleteAgentRequest {
            name: "reviewer".into(),
            expected_hash: None,
        },
        AgentChange {
            changed: true,
            file: None,
            refusal: None,
            problems: vec![],
            conflict: None,
        },
    );
    exchange::<RestoreAgent>(
        AgentName {
            name: "reviewer".into(),
        },
        AgentChange {
            changed: true,
            file: Some(reviewer_file()),
            refusal: None,
            problems: vec![],
            conflict: None,
        },
    );
    exchange::<AddShippedAgent>(
        AgentName {
            name: "worker".into(),
        },
        AgentChange {
            changed: false,
            file: None,
            refusal: Some("worker.md is already in the agents folder.".into()),
            problems: vec![],
            conflict: None,
        },
    );
    over_the_wire(
        &AgentsChange {
            names: vec!["reviewer".into(), "worker".into()],
        },
        |payload| {
            Message::Event(Event {
                subscription: 1,
                seq: 1,
                payload,
            })
        },
    );
}

#[test]
fn a_write_or_a_delete_may_leave_out_its_hash_and_its_rename() {
    let write: WriteAgentRequest = from_payload(
        &to_payload(&serde_json::json!({ "name": "new", "text": "---\n---\n" })).unwrap(),
    )
    .unwrap();
    assert_eq!((write.expected_hash, write.rename_from), (None, None));
    let delete: DeleteAgentRequest =
        from_payload(&to_payload(&serde_json::json!({ "name": "old" })).unwrap()).unwrap();
    assert_eq!(delete.expected_hash, None);
}
