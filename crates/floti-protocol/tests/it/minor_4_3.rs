//! A client on 4.2 still reads what a host on 4.3 sends, and that host still
//! reads what the client sends: protocol 4.3 adds the `desktop` capability
//! with `agentDesktop:call` and `agentDesktop:answer` (the floti-desktop
//! tools, which an older client never declares and so is never sent), the
//! host's optional `desktopMcp` setting, and `session:focus`'s optional
//! `active`, and changes nothing else.

use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::sessions::SessionFocusRequest;
use floti_protocol::channels::workspace::{DEFAULT_DESKTOP_MCP, HostSettings};
use floti_protocol::record::{Change, Difference, Part, differences, records, records_dir};
use floti_protocol::version::negotiate;
use floti_protocol::{Capability, CapabilitySet, PROTOCOL_VERSION, ProtocolVersion};
use serde_json::{Value, json};

const V4_2: ProtocolVersion = ProtocolVersion::new(4, 2);
const V4_3: ProtocolVersion = ProtocolVersion::new(4, 3);

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

fn difference(part: Part, name: &str, change: Change) -> Difference {
    Difference {
        part,
        name: name.to_owned(),
        change,
    }
}

#[test]
fn protocol_4_3_adds_the_desktop_relay_its_switch_and_the_windows_front_and_nothing_else() {
    assert!(PROTOCOL_VERSION >= V4_3);
    let mut found = differences(&record_text(V4_2), &record_text(V4_3)).unwrap();
    found.sort();
    let mut expected = vec![
        difference(Part::Channel, "agentDesktop:answer", Change::Added),
        difference(Part::Channel, "agentDesktop:call", Change::Added),
        difference(Part::Type, "AgentDesktopOutcome", Change::Added),
        difference(Part::Type, "AgentDesktopToolAnswer", Change::Added),
        difference(Part::Type, "AgentDesktopToolCall", Change::Added),
        difference(Part::Type, "Capability", Change::Changed),
        difference(Part::Type, "DesktopOpen", Change::Added),
        difference(Part::Type, "HostSettings", Change::Changed),
        difference(Part::Type, "HostSettingsPatch", Change::Changed),
        difference(Part::Type, "SessionFocusRequest", Change::Changed),
    ];
    expected.sort();
    assert_eq!(found, expected);

    let (before, after) = (record_of(V4_2), record_of(V4_3));
    // Each changed type changes only by what 4.3 adds.
    for (name, field) in [
        ("HostSettings", "desktopMcp"),
        ("HostSettingsPatch", "desktopMcp"),
        ("SessionFocusRequest", "active"),
    ] {
        let mut changed = after["types"][name].clone();
        changed["properties"]
            .as_object_mut()
            .unwrap()
            .remove(field)
            .unwrap_or_else(|| panic!("4.3 adds {name}.{field}"));
        if let Some(required) = changed.get("required").and_then(Value::as_array) {
            assert!(
                !required.iter().any(|each| each == field),
                "{name}.{field} is optional"
            );
        }
        assert_eq!(
            changed, before["types"][name],
            "{name} changes only by {field}"
        );
    }
    let mut capabilities = after["types"]["Capability"].clone();
    let values = capabilities["oneOf"].as_array_mut().unwrap();
    let added = values.iter().position(|value| value["const"] == "desktop");
    values.remove(added.expect("4.3 adds the desktop capability"));
    assert_eq!(capabilities, before["types"]["Capability"]);
}

#[test]
fn a_4_2_client_and_a_4_3_host_read_each_other() {
    assert_eq!(negotiate(V4_3, V4_2), Ok(V4_2));

    // A 4.2 client's focus has no `active`: read as a window that does not say.
    let older: SessionFocusRequest =
        from_payload(&to_payload(&json!({ "ids": [3] })).unwrap()).unwrap();
    assert_eq!(
        older,
        SessionFocusRequest {
            ids: vec![3],
            active: None
        }
    );

    // A 4.2 host's settings have no `desktopMcp`: read as on.
    let mut settings = serde_json::to_value(HostSettings {
        desktop_mcp: false,
        ..settings()
    })
    .unwrap();
    settings.as_object_mut().unwrap().remove("desktopMcp");
    let read: HostSettings = from_payload(&to_payload(&settings).unwrap()).unwrap();
    assert_eq!(read.desktop_mcp, DEFAULT_DESKTOP_MCP);
    assert!(read.desktop_mcp);

    // A client that names `desktop` to a host that does not know it has it
    // dropped, as every unknown name is.
    assert_eq!(
        CapabilitySet::from_names(["terminal", "desktop"]).to_vec(),
        [Capability::Terminal, Capability::Desktop]
    );
    assert_eq!(
        CapabilitySet::from_names(["terminal", "a name from later"]).to_vec(),
        [Capability::Terminal]
    );
}

fn settings() -> HostSettings {
    serde_json::from_value(json!({
        "scanRoots": [],
        "pinnedProjects": [],
        "firstRunCompletedAt": null,
        "backendPaths": {},
        "terminalShell": null,
        "transcriptArchiveMaxBytes": 1024,
        "browserMcp": true,
        "browserMcpLocalOnly": false,
        "sessionMcp": true,
        "restoreWithoutAsking": false,
    }))
    .unwrap()
}
