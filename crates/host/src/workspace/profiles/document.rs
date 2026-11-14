//! A profile as a file, so it can travel with a harness.
//!
//! The wire format is snake_case keys, with paths written relative to the
//! profile's own root wherever they are under it and the root itself written
//! against home (`~/.harness/dev`). That is what makes an export portable
//! rather than merely serialised: the same harness cloned somewhere else on
//! another machine still resolves `repos/acme`. Import reverses it against
//! the root, so a profile that has been through the file is the profile that
//! went in.
//!
//! The pin order is not part of the document. It is where this launcher puts
//! the profile in its own list, not anything about the launch, so imports
//! arrive unpinned.

use std::path::{Component, Path, PathBuf};

use floti_protocol::channels::common::PermissionMode;
use floti_protocol::channels::sessions::{EffortLevel, Profile, ProfileDraft};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_yaml_ng::Value;

use crate::agent_config::normalize;

/// The document written to disk, in the order a person reads it. An unset
/// key is written as `null` rather than left out: the export doubles as the
/// format a person edits by hand, and a listed key shows what can be set.
#[derive(Serialize)]
struct Document<'a> {
    name: &'a str,
    root: String,
    overlays: Vec<String>,
    access: Vec<String>,
    model: Option<&'a str>,
    effort: Option<EffortLevel>,
    permission_mode: Option<PermissionMode>,
    agent: Option<&'a str>,
    mcp: &'a [String],
    opening_prompt: Option<&'a str>,
    session_agents: Option<bool>,
    remote_control: Option<bool>,
}

/// `profile` as its YAML document. `home` is the user's home folder, which
/// the root is written against; with none, paths outside the root stay
/// absolute.
pub fn profile_to_yaml(profile: &ProfileDraft, home: Option<&Path>) -> String {
    let root = normalize(Path::new(&profile.root));
    let document = Document {
        name: &profile.name,
        root: contract_home(&root, home),
        overlays: contract_all(&profile.overlays, &root, home),
        access: contract_all(&profile.access, &root, home),
        model: profile.model.as_deref(),
        effort: profile.effort,
        permission_mode: profile.permission_mode,
        agent: profile.agent.as_deref(),
        mcp: &profile.mcp,
        opening_prompt: profile.opening_prompt.as_deref(),
        session_agents: profile.session_agents,
        remote_control: profile.remote_control,
    };
    // A struct of strings, lists of strings and plain enums always
    // serialises.
    serde_yaml_ng::to_string(&document).unwrap_or_default()
}

/// Parses an exported profile.
///
/// Refuses only a document that is not a profile at all. A field whose value
/// is not one the agent accepts, an `effort` of `medium-high` say, is dropped
/// rather than fatal: the user can see and fix it in the editor afterwards.
pub fn profile_from_yaml(text: &str, home: Option<&Path>) -> Result<ProfileDraft, String> {
    let parsed: Value = serde_yaml_ng::from_str(text)
        .map_err(|err| format!("Not a readable YAML document: {err}"))?;
    if !parsed.is_mapping() {
        return Err("A profile file must be a YAML mapping.".to_owned());
    }
    let name = nullable_string(parsed.get("name"));
    let raw_root = nullable_string(parsed.get("root"));
    let Some(name) = name else {
        return Err("A profile needs a `name`.".to_owned());
    };
    let Some(raw_root) = raw_root else {
        return Err("A profile needs a `root`.".to_owned());
    };

    let root = normalize(&expand_home(&raw_root, home));
    let paths = |key: &str| -> Vec<String> {
        string_list(parsed.get(key))
            .iter()
            .map(|path| expand_path(path, &root, home).display().to_string())
            .collect()
    };
    Ok(ProfileDraft {
        name,
        root: root.display().to_string(),
        overlays: paths("overlays"),
        access: paths("access"),
        model: nullable_string(parsed.get("model")),
        effort: named(parsed.get("effort")),
        permission_mode: named(parsed.get("permission_mode")),
        agent: nullable_string(parsed.get("agent")),
        mcp: string_list(parsed.get("mcp")),
        opening_prompt: nullable_string(parsed.get("opening_prompt")),
        pinned_order: None,
        // Only `false` is a choice; `true` and anything else follow the
        // host's setting.
        session_agents: match parsed.get("session_agents") {
            Some(Value::Bool(false)) => Some(false),
            _ => None,
        },
        // Only `true` is a choice; anything else is off.
        remote_control: match parsed.get("remote_control") {
            Some(Value::Bool(true)) => Some(true),
            _ => None,
        },
    })
}

/// A stored profile minus its identity: what export and equality care
/// about.
pub fn profile_draft(profile: &Profile) -> ProfileDraft {
    ProfileDraft {
        name: profile.name.clone(),
        root: profile.root.clone(),
        overlays: profile.overlays.clone(),
        access: profile.access.clone(),
        model: profile.model.clone(),
        effort: profile.effort,
        permission_mode: profile.permission_mode,
        agent: profile.agent.clone(),
        mcp: profile.mcp.clone(),
        opening_prompt: profile.opening_prompt.clone(),
        pinned_order: profile.pinned_order,
        session_agents: profile.session_agents,
        remote_control: profile.remote_control,
    }
}

/// Every problem with a draft, rather than the first: they are shown against
/// the fields of a form. The effort and permission mode are typed on the
/// wire, so a value the agent would not accept never reaches here.
pub fn validate_profile(draft: &ProfileDraft) -> Vec<String> {
    let mut problems = Vec::new();
    if draft.name.trim().is_empty() {
        problems.push("A profile needs a name.".to_owned());
    }
    let root = draft.root.trim();
    if root.is_empty() {
        problems.push("A profile needs a root directory.".to_owned());
    } else if !Path::new(root).is_absolute() {
        // Today's app resolved a relative root against its own working
        // folder, which names nothing the user chose; the host refuses it.
        problems.push("The root must be an absolute path.".to_owned());
    }
    problems
}

/// `path`'s components below `base`, or `None` when it is not under it.
/// Compared without case, as a Windows path is; a folder that differs from
/// another only in case is not something a profile names on purpose.
fn below(base: &Path, path: &Path) -> Option<Vec<String>> {
    let text = |part: Component<'_>| part.as_os_str().to_string_lossy().to_lowercase();
    let mut parts = path.components();
    for wanted in base.components() {
        match parts.next() {
            Some(part) if text(part) == text(wanted) => {}
            _ => return None,
        }
    }
    Some(
        parts
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect(),
    )
}

/// `~` for the home folder and `~/...` for anything under it; anything else
/// absolute.
fn contract_home(path: &Path, home: Option<&Path>) -> String {
    let absolute = normalize(path);
    match home.and_then(|home| below(&normalize(home), &absolute)) {
        Some(parts) if parts.is_empty() => "~".to_owned(),
        Some(parts) => format!("~/{}", parts.join("/")),
        None => absolute.display().to_string(),
    }
}

/// Under the root, written relative to it; anywhere else, against home.
fn contract_path(path: &str, root: &Path, home: Option<&Path>) -> String {
    let absolute = normalize(Path::new(path));
    match below(root, &absolute) {
        Some(parts) if parts.is_empty() => ".".to_owned(),
        Some(parts) => parts.join("/"),
        None => contract_home(&absolute, home),
    }
}

fn contract_all(paths: &[String], root: &Path, home: Option<&Path>) -> Vec<String> {
    paths
        .iter()
        .map(|path| contract_path(path, root, home))
        .collect()
}

fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    match home {
        Some(home) if path == "~" => home.to_path_buf(),
        Some(home) if path.starts_with("~/") || path.starts_with("~\\") => home.join(&path[2..]),
        _ => PathBuf::from(path),
    }
}

fn expand_path(path: &str, root: &Path, home: Option<&Path>) -> PathBuf {
    let expanded = expand_home(path, home);
    if expanded.is_absolute() {
        normalize(&expanded)
    } else {
        normalize(&root.join(expanded))
    }
}

fn nullable_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.trim().to_owned()),
        _ => None,
    }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A value the protocol names, such as `high` or `acceptEdits`, or `None`
/// for anything else.
fn named<T: DeserializeOwned>(value: Option<&Value>) -> Option<T> {
    match value {
        Some(Value::String(text)) => {
            serde_json::from_value(serde_json::Value::String(text.clone())).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use floti_protocol::channels::common::PermissionMode;
    use floti_protocol::channels::sessions::{EffortLevel, ProfileDraft};

    use super::{profile_from_yaml, profile_to_yaml, validate_profile};
    use crate::agent_config::normalize;

    /// An absolute home folder on this OS, with a space in it. Nothing reads
    /// or writes it: a document is written and read as text.
    fn home() -> PathBuf {
        std::env::temp_dir().join("floti profile home")
    }

    fn harness() -> PathBuf {
        home().join(".harness").join("dev")
    }

    fn text(path: &Path) -> String {
        path.display().to_string()
    }

    /// The spec's example profile, rooted at `~/.harness/dev`.
    fn sample() -> ProfileDraft {
        let harness = harness();
        ProfileDraft {
            name: "Acme cloud sync".into(),
            root: text(&harness),
            overlays: vec![
                text(&harness.join("repos").join("acme")),
                text(&harness.join("repos").join("acme-reporting")),
            ],
            access: vec![
                text(&harness.join("repos").join("acme")),
                text(&harness.join("repos").join("other-repo")),
            ],
            model: Some("opus".into()),
            effort: Some(EffortLevel::High),
            permission_mode: Some(PermissionMode::Auto),
            agent: None,
            mcp: vec!["clickup".into()],
            opening_prompt: Some("/recap".into()),
            pinned_order: None,
            session_agents: None,
            remote_control: None,
        }
    }

    // profile_to_yaml

    #[test]
    fn a_profile_document_writes_the_shape_the_spec_prints() {
        let yaml = profile_to_yaml(&sample(), Some(&home()));
        assert!(yaml.contains("name: Acme cloud sync"), "{yaml}");
        assert!(yaml.contains("permission_mode: auto"), "{yaml}");
        assert!(yaml.contains("opening_prompt: /recap"), "{yaml}");
        assert!(!yaml.contains("permissionMode"), "{yaml}");
    }

    #[test]
    fn a_profile_document_writes_paths_under_the_root_relative_to_it_and_the_root_against_home() {
        let yaml = profile_to_yaml(&sample(), Some(&home()));
        assert!(yaml.contains("repos/acme"), "{yaml}");
        assert!(yaml.contains("~/.harness/dev"), "{yaml}");
        assert!(
            !yaml.contains(&text(&harness().join("repos").join("acme"))),
            "{yaml}"
        );
    }

    #[test]
    fn a_profile_document_keeps_a_path_outside_the_root_addressable() {
        let yaml = profile_to_yaml(
            &ProfileDraft {
                access: vec!["C:/elsewhere/repo".into()],
                ..sample()
            },
            Some(&home()),
        );
        assert!(yaml.contains("elsewhere"), "{yaml}");
    }

    #[test]
    fn a_profile_document_lists_the_unset_keys_rather_than_dropping_them() {
        let yaml = profile_to_yaml(
            &ProfileDraft {
                agent: None,
                model: None,
                ..sample()
            },
            Some(&home()),
        );
        assert!(yaml.contains("agent: null"), "{yaml}");
        assert!(yaml.contains("model: null"), "{yaml}");
    }

    #[test]
    fn a_profile_document_does_not_carry_the_pin_order_so_imports_arrive_unpinned() {
        let home = home();
        let home = Some(home.as_path());
        let yaml = profile_to_yaml(
            &ProfileDraft {
                pinned_order: Some(3),
                ..sample()
            },
            home,
        );
        assert!(!yaml.contains("pinned"), "{yaml}");
        assert_eq!(profile_from_yaml(&yaml, home).unwrap().pinned_order, None);
    }

    // profile_from_yaml

    #[test]
    fn a_profile_round_trips_through_its_document_intact() {
        let home = home();
        let home = Some(home.as_path());
        let profile = sample();
        assert_eq!(
            profile_from_yaml(&profile_to_yaml(&profile, home), home).unwrap(),
            profile
        );
    }

    #[test]
    fn a_profile_with_everything_unset_round_trips_too() {
        let home = home();
        let home = Some(home.as_path());
        let bare = ProfileDraft {
            name: "bare".into(),
            root: text(&harness()),
            overlays: Vec::new(),
            access: Vec::new(),
            model: None,
            effort: None,
            permission_mode: None,
            agent: None,
            mcp: Vec::new(),
            opening_prompt: None,
            pinned_order: None,
            session_agents: None,
            remote_control: None,
        };
        assert_eq!(
            profile_from_yaml(&profile_to_yaml(&bare, home), home).unwrap(),
            bare
        );
    }

    #[test]
    fn a_profile_that_turns_the_agents_tools_off_says_so_in_its_document_and_keeps_it_through_an_import()
     {
        let home = home();
        let home = Some(home.as_path());
        let off = ProfileDraft {
            session_agents: Some(false),
            ..sample()
        };
        let yaml = profile_to_yaml(&off, home);
        assert!(yaml.contains("session_agents: false"), "{yaml}");
        assert_eq!(profile_from_yaml(&yaml, home).unwrap(), off);

        // One that follows the setting lists the key unset, like the others.
        let following = profile_to_yaml(&sample(), home);
        assert!(following.contains("session_agents: null"), "{following}");

        // A document written by hand that says true follows the setting too.
        let yaml = yaml.replace("session_agents: false", "session_agents: true");
        assert_eq!(profile_from_yaml(&yaml, home).unwrap().session_agents, None);
    }

    #[test]
    fn a_profile_document_says_whether_remote_control_is_on() {
        let home = home();
        let home = Some(home.as_path());
        let on = ProfileDraft {
            remote_control: Some(true),
            ..sample()
        };
        let yaml = profile_to_yaml(&on, home);
        assert!(yaml.contains("remote_control: true"), "{yaml}");
        assert_eq!(profile_from_yaml(&yaml, home).unwrap(), on);

        // One without it lists the key unset, like the others, and reads as off.
        let plain = profile_to_yaml(&sample(), home);
        assert!(plain.contains("remote_control: null"), "{plain}");
        assert_eq!(
            profile_from_yaml(&plain, home).unwrap().remote_control,
            None
        );
        let off = yaml.replace("remote_control: true", "remote_control: false");
        assert_eq!(profile_from_yaml(&off, home).unwrap().remote_control, None);
    }

    #[test]
    fn relative_paths_resolve_against_the_root_so_a_moved_harness_still_works() {
        let imported = profile_from_yaml(
            "name: moved\nroot: C:/somewhere/else\noverlays:\n  - repos/acme\n",
            None,
        )
        .unwrap();
        assert_eq!(
            imported.overlays,
            [text(&normalize(Path::new("C:/somewhere/else/repos/acme")))]
        );
    }

    #[test]
    fn a_value_the_agent_would_reject_is_dropped_rather_than_refusing_the_file() {
        let imported = profile_from_yaml(
            &format!(
                "name: odd\nroot: {}\neffort: medium-high\npermission_mode: whatever\n",
                text(&harness()).replace('\\', "/")
            ),
            None,
        )
        .unwrap();
        assert_eq!(imported.effort, None);
        assert_eq!(imported.permission_mode, None);
    }

    #[test]
    fn a_document_that_is_not_a_profile_is_refused() {
        let refusal = |yaml: &str| profile_from_yaml(yaml, None).unwrap_err();
        assert!(refusal("- a\n- b").contains("mapping"));
        assert!(refusal("root: /harness").contains("name"));
        assert!(refusal("name: x").contains("root"));
        assert!(refusal("name: [unclosed").contains("YAML"));
    }

    // validate_profile

    #[test]
    fn a_good_draft_validates() {
        assert_eq!(validate_profile(&sample()), Vec::<String>::new());
    }

    #[test]
    fn validation_reports_every_problem_at_once() {
        let problems = validate_profile(&ProfileDraft {
            name: "  ".into(),
            root: " ".into(),
            ..sample()
        });
        assert_eq!(problems.len(), 2, "{problems:?}");
    }
}
