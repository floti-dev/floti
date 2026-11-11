//! Protocol 3.0 is a major: it removes the channels only Config's page used
//! (and the copy of its file versions), the types only they carried, and the
//! three fields 2.x retired, adds only a file view's optional `readOnly`, and
//! changes nothing else. What composition reads of an agent's config stays:
//! `config:scopes`, `config:tree` and `config:effective`.

use std::collections::BTreeSet;

use floti_protocol::record::{Change, Difference, Part, differences, records, records_dir};
use floti_protocol::version::negotiate;
use floti_protocol::{PROTOCOL_VERSION, ProtocolVersion, UpdateSide};
use serde_json::Value;

const V2_3: ProtocolVersion = ProtocolVersion::new(2, 3);
const V3_0: ProtocolVersion = ProtocolVersion::new(3, 0);

/// Every channel 3.0 takes away.
const REMOVED_CHANNELS: &[&str] = &[
    "carry:versions",
    "config:create",
    "config:delete",
    "config:doctor",
    "config:externalChange",
    "config:mcpAdd",
    "config:mcpApprove",
    "config:mcpList",
    "config:mcpPreview",
    "config:mcpRemove",
    "config:read",
    "config:rename",
    "config:render",
    "config:restore",
    "config:snapshot",
    "config:snapshots",
    "config:watch",
    "config:write",
];

/// Every type 3.0 takes away: those only the removed channels carried.
const REMOVED_TYPES: &[&str] = &[
    "CarriedConfigVersion",
    "CarriedConfigVersions",
    "ConfigExternalChange",
    "ConfigPath",
    "ConfigRendered",
    "CreatableKind",
    "CreateConfigRequest",
    "CreateConfigResult",
    "DeleteConfigRequest",
    "DeleteConfigResult",
    "DeletedConfigFile",
    "DoctorReport",
    "DoctorRow",
    "McpAddRequest",
    "McpApproveRequest",
    "McpDiffLine",
    "McpDiffSign",
    "McpListRequest",
    "McpPreview",
    "McpRemoveRequest",
    "McpResult",
    "MovedFile",
    "RenameConfigRequest",
    "RenameConfigResult",
    "RenderedCode",
    "SnapshotContent",
    "SnapshotRef",
    "WatchConfigRequest",
];

/// The types that lose a retired field, and the field each loses.
const RETIRED_FIELDS: &[(&str, &str)] = &[
    ("HostSettings", "agentHeavyPerHost"),
    ("HostSettingsPatch", "agentHeavyPerHost"),
    ("SessionRecord", "role"),
];

fn record_text(version: ProtocolVersion) -> String {
    records(&records_dir())
        .unwrap()
        .into_iter()
        .find_map(|(each, text)| (each == version).then_some(text))
        .unwrap_or_else(|| panic!("there is a record of {version}"))
}

fn record_of(version: ProtocolVersion) -> Value {
    serde_json::from_str(&record_text(version)).unwrap()
}

fn names(found: &[Difference], part: Part, change: Change) -> Vec<&str> {
    found
        .iter()
        .filter(|difference| difference.part == part && difference.change == change)
        .map(|difference| difference.name.as_str())
        .collect()
}

#[test]
fn protocol_3_0_removes_the_config_pages_channels_their_types_and_the_retired_fields_and_adds_only_a_file_views_read_only()
 {
    assert_eq!(PROTOCOL_VERSION, V3_0);
    let found = differences(&record_text(V2_3), &record_text(V3_0)).unwrap();
    assert_eq!(
        names(&found, Part::Channel, Change::Added),
        Vec::<&str>::new()
    );
    assert_eq!(names(&found, Part::Type, Change::Added), Vec::<&str>::new());
    assert_eq!(
        names(&found, Part::Channel, Change::Changed),
        Vec::<&str>::new()
    );
    assert_eq!(
        names(&found, Part::Channel, Change::Removed),
        REMOVED_CHANNELS
    );
    assert_eq!(names(&found, Part::Type, Change::Removed), REMOVED_TYPES);
    assert_eq!(
        names(&found, Part::Type, Change::Changed),
        [
            "FileView",
            "HostSettings",
            "HostSettingsPatch",
            "NoticeLimit",
            "SessionRecord"
        ]
    );

    // A file view only gains an optional `readOnly`, so a view without it
    // reads as before.
    let (before, after) = (record_of(V2_3), record_of(V3_0));
    let mut view = after["types"]["FileView"].clone();
    view["properties"]
        .as_object_mut()
        .unwrap()
        .remove("readOnly")
        .expect("3.0's FileView has readOnly");
    assert_eq!(
        view, before["types"]["FileView"],
        "FileView gains only readOnly"
    );

    // Each type that changes only loses its retired field.
    for (name, field) in RETIRED_FIELDS {
        let mut was = before["types"][*name].clone();
        was["properties"]
            .as_object_mut()
            .unwrap()
            .remove(*field)
            .unwrap_or_else(|| panic!("2.3's {name} has {field}"));
        if let Some(required) = was.get_mut("required").and_then(Value::as_array_mut) {
            required.retain(|each| each != field);
        }
        assert_eq!(after["types"][*name], was, "{name} loses only {field}");
    }
    // A notice's limit loses only `heavy`.
    let limits = |record: &Value| -> BTreeSet<String> {
        serde_json::to_string(&record["types"]["NoticeLimit"])
            .unwrap()
            .split('"')
            .filter(|word| ["children", "sessions", "depth", "starts", "heavy"].contains(word))
            .map(str::to_owned)
            .collect()
    };
    let mut kept = limits(&before);
    assert!(kept.remove("heavy"), "2.3 has the heavy limit");
    assert_eq!(limits(&after), kept);
}

#[test]
fn what_composition_reads_of_an_agents_config_stays() {
    let channels = record_of(V3_0)["channels"].clone();
    for kept in ["config:scopes", "config:tree", "config:effective"] {
        assert_eq!(
            channels[kept],
            record_of(V2_3)["channels"][kept],
            "{kept} is as it was"
        );
    }
    let left: Vec<&String> = channels
        .as_object()
        .unwrap()
        .keys()
        .filter(|name| name.starts_with("config:"))
        .collect();
    assert_eq!(left, ["config:effective", "config:scopes", "config:tree"]);
}

#[test]
fn a_3_0_host_refuses_a_2_3_client_saying_to_update_it() {
    let refused = negotiate(V3_0, V2_3).unwrap_err();
    assert_eq!(refused.update, UpdateSide::Client);
    assert_eq!(negotiate(V3_0, V3_0), Ok(V3_0));
}
