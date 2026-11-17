//! Protocol 4.2 adds `openingPrompt` to `session:launch`: the session's first
//! message, which the guided first run's last step sends. It is optional, so
//! a 4.1 launch without it reads as before and a host before it ignores it.

use floti_protocol::channels::sessions::LaunchSessionRequest;
use floti_protocol::{PROTOCOL_VERSION, ProtocolVersion};
use serde_json::json;

#[test]
fn a_launch_says_its_first_message_only_when_it_has_one_and_one_without_reads_as_none() {
    assert!(PROTOCOL_VERSION >= ProtocolVersion::new(4, 2));
    let before: LaunchSessionRequest = serde_json::from_value(json!({
        "cwd": "/work/my-work",
        "profileId": null,
        "permissionMode": null,
        "resume": null,
        "cols": 80,
        "rows": 24
    }))
    .expect("a 4.1 launch, which names no first message");
    assert_eq!(before.opening_prompt, None);
    assert!(serde_json::to_value(&before).unwrap().get("openingPrompt").is_none());

    let with = LaunchSessionRequest {
        opening_prompt: Some("Read AGENTS.md and tell me what this workspace is for.".to_owned()),
        ..before
    };
    assert_eq!(
        serde_json::to_value(&with).unwrap()["openingPrompt"],
        "Read AGENTS.md and tell me what this workspace is for."
    );
}
