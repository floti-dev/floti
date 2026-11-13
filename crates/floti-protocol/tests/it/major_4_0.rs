//! Protocol 4.0 is a major: harnesses are called workspaces. `harness:create`
//! is `workspace:create`, every type named for a harness is named for a
//! workspace, and every field and value that said harness says workspace (a
//! project's `workspacePath`, a discovery's `workspaces`, the project kind,
//! config scope kind and template kind `workspace`). Nothing else changes, so
//! 4.0's record is 3.1's with the one word renamed.

use floti_protocol::record::{Change, Difference, Part, differences, records, records_dir};
use floti_protocol::version::negotiate;
use floti_protocol::{PROTOCOL_VERSION, ProtocolVersion, UpdateSide};
use serde_json::Value;

const V3_1: ProtocolVersion = ProtocolVersion::new(3, 1);
const V4_0: ProtocolVersion = ProtocolVersion::new(4, 0);

/// The types 3.1 named for a harness, each beside the one 4.0 names for a
/// workspace in its place.
const RENAMED_TYPES: &[(&str, &str)] = &[
    ("CreateHarnessOutcome", "CreateWorkspaceOutcome"),
    ("CreateHarnessRequest", "CreateWorkspaceRequest"),
    ("Harness", "Workspace"),
    ("HarnessMode", "WorkspaceMode"),
    ("HarnessPart", "WorkspacePart"),
    ("HarnessSupport", "WorkspaceSupport"),
];

fn record_text(version: ProtocolVersion) -> String {
    records(&records_dir())
        .unwrap()
        .into_iter()
        .find_map(|(each, text)| (each == version).then_some(text))
        .unwrap_or_else(|| panic!("there is a record of {version}"))
}

/// Every spelling of the word, as the rename changed it.
fn renamed(text: &str) -> String {
    [
        ("harnesses", "workspaces"),
        ("Harnesses", "Workspaces"),
        ("harness", "workspace"),
        ("Harness", "Workspace"),
    ]
    .iter()
    .fold(text.to_owned(), |text, (old, new)| text.replace(old, new))
}

/// A record's contents, without the version it names and the sentence that
/// names it.
fn without_version(text: &str) -> Value {
    let mut record: Value = serde_json::from_str(text).unwrap();
    let fields = record.as_object_mut().unwrap();
    fields.remove("version");
    fields.remove("description");
    record
}

fn names(found: &[Difference], part: Part, change: Change) -> Vec<&str> {
    found
        .iter()
        .filter(|difference| difference.part == part && difference.change == change)
        .map(|difference| difference.name.as_str())
        .collect()
}

#[test]
fn protocol_4_0_is_3_1_with_harness_renamed_workspace_and_nothing_else() {
    assert!(PROTOCOL_VERSION.major >= V4_0.major);
    let (before, after) = (record_text(V3_1), record_text(V4_0));
    assert!(
        !after.to_lowercase().contains("harness"),
        "4.0 still says harness somewhere"
    );
    assert_eq!(
        without_version(&after),
        without_version(&renamed(&before)),
        "4.0 differs from 3.1 by more than the word"
    );

    let found = differences(&before, &after).unwrap();
    assert_eq!(
        names(&found, Part::Channel, Change::Removed),
        ["harness:create"]
    );
    assert_eq!(
        names(&found, Part::Channel, Change::Added),
        ["workspace:create"]
    );
    let mut removed: Vec<&str> = RENAMED_TYPES.iter().map(|(old, _)| *old).collect();
    let mut added: Vec<&str> = RENAMED_TYPES.iter().map(|(_, new)| *new).collect();
    removed.sort_unstable();
    added.sort_unstable();
    assert_eq!(names(&found, Part::Type, Change::Removed), removed);
    assert_eq!(names(&found, Part::Type, Change::Added), added);
}

#[test]
fn a_4_0_host_refuses_a_3_1_client_and_a_3_1_host_refuses_a_4_0_client_each_saying_which_to_update()
{
    let refused = negotiate(V4_0, V3_1).unwrap_err();
    assert_eq!(refused.update, UpdateSide::Client);
    let refused = negotiate(V3_1, V4_0).unwrap_err();
    assert_eq!(refused.update, UpdateSide::Host);
    assert_eq!(negotiate(V4_0, V4_0), Ok(V4_0));
}
