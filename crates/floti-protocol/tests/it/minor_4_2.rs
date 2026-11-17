//! A client on 4.1 still reads what a host on 4.2 sends: protocol 4.2 adds a
//! project inventory's optional `agentsMd` and `agentsDir` (a workspace
//! written in the shared shape, `AGENTS.md` and `.agents/`) and a config
//! file's `agents-md` kind, and changes nothing else. A 4.2 inventory is a
//! 4.1 inventory with fields the older reader ignores.

use floti_protocol::channel::{from_payload, to_payload};
use floti_protocol::channels::workspace::ClaudeInventory;
use floti_protocol::record::{Change, Difference, Part, differences, records, records_dir};
use floti_protocol::version::negotiate;
use floti_protocol::{PROTOCOL_VERSION, ProtocolVersion};
use serde::Deserialize;
use serde_json::Value;

const V4_1: ProtocolVersion = ProtocolVersion::new(4, 1);
const V4_2: ProtocolVersion = ProtocolVersion::new(4, 2);

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
fn protocol_4_2_adds_only_the_inventorys_optional_shared_shape_fields_and_the_agents_md_kind() {
    assert!(PROTOCOL_VERSION >= V4_2);
    let found = differences(&record_text(V4_1), &record_text(V4_2)).unwrap();
    assert_eq!(
        found,
        vec![
            Difference {
                part: Part::Type,
                name: "ClaudeInventory".to_owned(),
                change: Change::Changed,
            },
            Difference {
                part: Part::Type,
                name: "ConfigFileKind".to_owned(),
                change: Change::Changed,
            },
        ]
    );
    let (before, after) = (record_of(V4_1), record_of(V4_2));
    let mut inventory = after["types"]["ClaudeInventory"].clone();
    let properties = inventory["properties"].as_object_mut().unwrap();
    for added in ["agentsMd", "agentsDir"] {
        properties
            .remove(added)
            .unwrap_or_else(|| panic!("4.2 adds ClaudeInventory.{added}"));
    }
    assert_eq!(
        inventory, before["types"]["ClaudeInventory"],
        "ClaudeInventory changes only by its optional fields, so a 4.1 inventory is a whole 4.2 one"
    );
    let mut kinds = after["types"]["ConfigFileKind"].clone();
    let values = kinds["enum"].as_array_mut().unwrap();
    let added = values.iter().position(|value| value == "agents-md");
    values.remove(added.expect("4.2 adds the agents-md kind"));
    assert_eq!(
        kinds, before["types"]["ConfigFileKind"],
        "ConfigFileKind changes only by its added value"
    );
}

/// An inventory as a 4.1 client reads it: without the shared shape, and not
/// strictly.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeInventoryV41 {
    skills: u32,
    claude_md: bool,
}

#[test]
fn a_4_1_client_reads_a_4_2_inventory_and_a_4_2_client_reads_a_4_1_one() {
    assert_eq!(negotiate(V4_2, V4_1), Ok(V4_1));
    let shared = ClaudeInventory {
        skills: 3,
        commands: 0,
        agents: 0,
        hooks: false,
        settings: false,
        claude_md: false,
        mcp: false,
        agents_md: true,
        agents_dir: true,
    };
    let old: ClaudeInventoryV41 =
        from_payload(&to_payload(&shared).unwrap()).expect("a 4.1 client reads it");
    assert_eq!((old.skills, old.claude_md), (3, false));

    let mut wire = serde_json::to_value(shared).unwrap();
    let fields = wire.as_object_mut().unwrap();
    fields.remove("agentsMd");
    fields.remove("agentsDir");
    let from_old_host: ClaudeInventory = from_payload(&to_payload(&wire).unwrap()).unwrap();
    assert!(!from_old_host.agents_md && !from_old_host.agents_dir);
}
