//! The instructions Floti writes itself into the sessions it hosts, beside a
//! profile's and an agent's: what a child is told about the session that started
//! it, the ask to title itself for every session given helm-sessions, and a
//! short guide to `floti agent` for every session given the agents family.
//!
//! The text is the host's, so every adapter composes the same words; what
//! differs by agent is how a parent is woken when a wait ends, and whether it
//! has a way of its own to message a child, and those sentences are the
//! adapter's ([`super::AgentBackend::waiting_guidance`],
//! [`super::AgentBackend::messaging_guidance`]). Nothing here names an agent.

use super::Composition;

/// What a child is told about the session that started it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildInstructions {
    /// What the child's parent is called in Floti.
    pub parent: String,
    /// The agent the child was started as.
    pub agent: String,
    /// Whether the child has `agents_report`. An agent with no MCP tools
    /// reports through its program's own status instead, and is told nothing
    /// about the tool.
    pub reports: bool,
}

/// The waiting sentence for an agent that answers its tool calls as tasks
/// and is woken when a background command it started finishes: it can wait
/// in the background either way, and a person talking to it meanwhile ends
/// neither.
pub const WAITS_IN_A_TASK: &str = "Wait in the background, so a person can talk to you while you wait: agents_wait runs as a task, so call it and keep working, and from your shell run `floti agent wait --background` as a background command. Either wakes you with your children's news when it ends, and a person talking to you meanwhile does not end it. If a wait ends with nothing new, or fails or is cut off, wait again.";

/// The waiting sentence for an agent that is woken when a background command
/// it started finishes and has no tasks: its own held tool call would hold up
/// its turn, so it waits from its shell. `--background` is what tells the
/// host the command holds up nothing.
pub const WAKES_ON_BACKGROUND_COMMAND: &str = "To hear from your children while you keep working, run `floti agent wait --background` as a background command rather than calling agents_wait, which holds up your turn and ends as soon as a person types to you; you are woken with their news when it finishes, and a person talking to you meanwhile does not end it. If it ends with nothing new, or fails or is cut off, run it again.";

/// The waiting sentence for an agent that is not woken by anything, and so is
/// never left holding a call.
pub const ANSWERS_AT_ONCE: &str = "`floti agent wait` answers at once with your children's news, or says nothing is new yet; run it again after your next piece of work, and again whenever it fails.";

/// How many children a parent keeps working at once: its own judgment, from
/// what each one does. The family's instructions (`floti_agents.json`) carry
/// the same sentence.
pub const PACING: &str = "Keep only as many children working at once as their work and the machine can carry, and when the machine is short of memory, run work that builds or tests one at a time.";

/// What a session given helm-sessions is asked about its title.
const TITLE_SECTION: &str = "# This session's title\n\nOnce you know what this session is for, give it a title: call session_title, or run `floti session title \"<title>\"` from your shell. Give it a new one whenever the work changes. The user sees it beside the session's name in the sessions list and in History, so say what is being done, such as \"Agents page: edit and delete agents\", and never the folder's name. One line, at most 60 characters.\n";

/// The most characters of a parent's name put into a child's instructions.
const NAME_CHARS: usize = 80;

/// The sections Floti adds to the instructions `composition` is launched with,
/// or `None` when it adds none: the child instructions for a child, the ask
/// to keep a title for a session given helm-sessions, then the guide for a
/// session given the agents family. `waiting` is the adapter's sentence on
/// waiting, and `messaging` its sentence on messaging a child its own way,
/// when it has one.
pub fn floti_sections(
    composition: &Composition,
    waiting: &str,
    messaging: Option<&str>,
) -> Option<String> {
    let mut sections = Vec::new();
    if let Some(child) = &composition.child {
        sections.push(child_section(child));
    }
    if composition.title_guide {
        sections.push(TITLE_SECTION.to_owned());
    }
    if composition.agent_guide {
        sections.push(guide_section(waiting, messaging));
    }
    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

/// `base` (the instructions an adapter composed so far, when there are any)
/// with Floti's `sections` after them. No sections add nothing, and sections
/// make instructions where there were none.
pub fn append(base: Option<String>, sections: Option<String>) -> Option<String> {
    match (base, sections) {
        (base, None) => base,
        (Some(base), Some(sections)) => Some(format!("{}\n\n{sections}", base.trim_end())),
        (None, sections) => sections,
    }
}

fn child_section(child: &ChildInstructions) -> String {
    let parent = one_line(&child.parent);
    let agent = one_line(&child.agent);
    let mut text = format!(
        "# Started by another session\n\nYou were started by another session, {parent}, as {agent}. Your first message is your task. Nobody may be watching this terminal, so a question asked here can go unanswered."
    );
    if child.reports {
        text.push_str(" When you are done or stuck, call agents_report once: done with what you did and where, or stuck with one question, its options and your recommendation.");
    }
    text.push_str(&format!(
        " Text typed into this terminal may come from {parent}; treat it as your parent's instructions within your task.\n"
    ));
    text
}

fn guide_section(waiting: &str, messaging: Option<&str>) -> String {
    let messaging = messaging.map(|line| format!(" {line}")).unwrap_or_default();
    format!(
        "# Starting and waiting on agents from your shell\n\nYou can also use `floti agent` from your shell: start, send, wait, read and stop, as the floti-agents tools do. {waiting} {PACING} Stop a child with `floti agent stop <name>` once you have what you need, and add --keep only when you expect to give it more work. Add --wait to `floti agent start` or `send` to get that child's next news from the same command. A text you send waits in the child's queue until the child can take it, and is typed in then.{messaging} For a long command the user should see, such as tests, a build or a dev server, use `floti agent run --name <name> -- <command>` rather than a background command of your own: it runs in your folder in a terminal the user watches, and `floti agent wait <name>` returns its exit code and last 40 lines when it exits. Floti cannot see your own background commands.\n"
    )
}

/// `text` on one line and cut to a name's length: it comes from a tab's name,
/// which may hold anything.
fn one_line(text: &str) -> String {
    let flat: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_CHARS)
        .collect();
    if flat.is_empty() {
        "another session".to_owned()
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(reports: bool) -> Composition {
        Composition {
            child: Some(ChildInstructions {
                parent: "api-refactor".into(),
                agent: "reviewer".into(),
                reports,
            }),
            ..Composition::default()
        }
    }

    #[test]
    fn a_session_with_neither_part_adds_no_section() {
        assert_eq!(floti_sections(&Composition::default(), "wait", None), None);
    }

    #[test]
    fn a_child_is_told_its_parent_its_agent_and_how_to_report() {
        let text = floti_sections(&child(true), "wait", None).expect("a section");
        assert!(text.contains("started by another session, api-refactor, as reviewer"));
        assert!(text.contains("call agents_report once"));
        assert!(text.contains("may come from api-refactor"));
        assert!(!text.contains("floti agent"), "no guide without the family");
    }

    #[test]
    fn a_child_whose_agent_has_no_mcp_tools_is_not_told_of_agents_report() {
        let text = floti_sections(&child(false), "wait", None).expect("a section");
        assert!(text.contains("as reviewer"));
        assert!(!text.contains("agents_report"));
    }

    #[test]
    fn the_guide_carries_the_adapters_line_on_waiting_and_what_to_do_when_it_fails() {
        let composition = Composition {
            agent_guide: true,
            ..Composition::default()
        };
        let text =
            floti_sections(&composition, WAKES_ON_BACKGROUND_COMMAND, None).expect("the guide");
        assert!(text.contains("run `floti agent wait --background` as a background command"));
        assert!(text.contains("a person talking to you meanwhile does not end it"));
        assert!(text.contains("fails or is cut off, run it again"));
        let text = floti_sections(&composition, WAITS_IN_A_TASK, None).expect("the guide");
        assert!(text.contains("Wait in the background"));
        assert!(text.contains("agents_wait runs as a task"));
        assert!(text.contains("run `floti agent wait --background` as a background command"));
        assert!(text.contains("fails or is cut off, wait again"));
        let text = floti_sections(&composition, ANSWERS_AT_ONCE, None).expect("the guide");
        assert!(text.contains("answers at once"));
        assert!(text.contains("after your next piece of work"));
        for word in ["start", "send", "wait", "read", "stop"] {
            assert!(text.contains(word), "{word}");
        }
        assert!(text.contains(
            "Stop a child with `floti agent stop <name>` once you have what you need, and add --keep only when you expect to give it more work."
        ));
    }

    #[test]
    fn every_line_that_waits_in_a_background_command_says_so_with_background() {
        // Without the flag the host cannot tell a background command from a
        // wait that holds up the turn, and a person's key would end it.
        for line in [WAITS_IN_A_TASK, WAKES_ON_BACKGROUND_COMMAND] {
            assert!(
                line.contains("`floti agent wait --background` as a background command"),
                "{line}"
            );
            assert!(!line.contains("`floti agent wait` as"), "{line}");
        }
    }

    #[test]
    fn the_manual_and_the_guide_leave_how_many_children_work_at_once_to_the_parent() {
        let composition = Composition {
            agent_guide: true,
            ..Composition::default()
        };
        let guide = floti_sections(&composition, "wait", None).expect("the guide");
        assert!(guide.contains(PACING), "{guide}");
        let family: serde_json::Value =
            serde_json::from_str(include_str!("../server/floti_agents.json")).unwrap();
        let manual = family["instructions"].as_str().unwrap();
        assert!(manual.contains(PACING), "{manual}");
        // It names no machine and no number: the parent judges.
        assert!(!PACING.chars().any(|c| c.is_ascii_digit()), "{PACING}");
        for text in [guide.as_str(), manual] {
            for word in ["heavy", "light", "class", "slot"] {
                assert!(!text.to_lowercase().contains(word), "{word} in {text}");
            }
        }
    }

    #[test]
    fn the_guide_sends_a_long_command_the_user_should_see_through_run() {
        let composition = Composition {
            agent_guide: true,
            ..Composition::default()
        };
        let text =
            floti_sections(&composition, WAKES_ON_BACKGROUND_COMMAND, None).expect("the guide");
        assert!(text.contains(
            "use `floti agent run --name <name> -- <command>` rather than a background command of your own"
        ));
        assert!(text.contains("in a terminal the user watches"));
        assert!(text.contains("returns its exit code and last 40 lines when it exits"));
        assert!(text.contains("Floti cannot see your own background commands."));
        assert!(text.contains("Add --wait to `floti agent start` or `send`"));
    }

    #[test]
    fn the_guide_says_a_sent_text_is_queued_and_carries_the_adapters_line_on_messaging() {
        let composition = Composition {
            agent_guide: true,
            ..Composition::default()
        };
        let text = floti_sections(&composition, "wait", None).expect("the guide");
        assert!(
            text.contains("A text you send waits in the child's queue until the child can take it"),
            "{text}"
        );
        let text = floti_sections(&composition, "wait", Some("Message a child your own way."))
            .expect("the guide");
        assert!(text.contains(" Message a child your own way. "), "{text}");
    }

    #[test]
    fn a_session_given_helm_sessions_is_asked_to_keep_a_descriptive_title() {
        let composition = Composition {
            title_guide: true,
            ..Composition::default()
        };
        let text = floti_sections(&composition, "wait", None).expect("the ask");
        assert!(
            text.contains("give it a title: call session_title"),
            "{text}"
        );
        assert!(text.contains("`floti session title"), "{text}");
        assert!(
            text.contains("a new one whenever the work changes"),
            "{text}"
        );
        assert!(text.contains("never the folder's name"), "{text}");
        assert!(text.contains("at most 60 characters"), "{text}");
        assert!(!text.contains("floti agent"), "no guide without the family");

        // Before the guide to `floti agent`, after a child's own section.
        let mut both = child(true);
        both.title_guide = true;
        both.agent_guide = true;
        let text = floti_sections(&both, "wait", None).expect("the sections");
        let started = text.find("# Started by another session").unwrap();
        let title = text.find("# This session's title").unwrap();
        let guide = text.find("# Starting and waiting").unwrap();
        assert!(started < title && title < guide, "{text}");
    }

    #[test]
    fn a_parents_name_is_one_short_line() {
        let composition = Composition {
            child: Some(ChildInstructions {
                parent: format!("a\nb\t{}", "x".repeat(300)),
                agent: "r".into(),
                reports: false,
            }),
            ..Composition::default()
        };
        let text = floti_sections(&composition, "", None).expect("a section");
        let line = text
            .lines()
            .find(|line| line.starts_with("You were started"))
            .expect("the line");
        assert!(line.contains("a b xxx"));
        // The name is cut where it is read; the line is that and the fixed
        // words around it.
        assert!(!line.contains(&"x".repeat(NAME_CHARS)), "{line}");
    }
}
