//! `floti desktop`: the floti-desktop family from a session's own shell.
//!
//! `floti desktop open <view> ...` opens one of Floti's views beside the
//! session's tab, as its `desktop_open` tool does: `file <path> [--line <n>]`,
//! `folder <path>`, `project <path>`, `history [<search>...]` and
//! `settings [<section>]`. Each subcommand is one tool of the family, called
//! over the same endpoint the session's agent tools use, so the host answers
//! one way whoever asks and the same folder rule applies. The session is
//! known by the bearer token in `FLOTI_AGENT_TOKEN` and the address in
//! `FLOTI_AGENT_DESKTOP_URL`, which the host puts in the environment of every
//! session it gave the family. Nothing here names a session: the token is the
//! whole of who is asking.
//!
//! A relative path is made absolute against the shell's own folder before it
//! is sent, so it means what it means where the command runs, which is the
//! session's folder until the shell moves.
//!
//! A refusal is printed to standard error and the exit status is 1, so a
//! script and a model both see it; with `--json` the answer is the tool's
//! JSON.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::agent_cli::{Family, Outcome, TOKEN_VAR, address_and_token, call, print_outcome};

/// The variable that carries the address of the session's floti-desktop
/// family. Its name starts as the agents family's do, so what the host strips
/// from an inherited environment strips it too.
pub const DESKTOP_URL_VAR: &str = "FLOTI_AGENT_DESKTOP_URL";

/// What `floti desktop` accepts.
pub const USAGE: &str = "usage: floti desktop open file <path> [--line <n>] [--json]\n       floti desktop open folder <path> [--json]\n       floti desktop open project <path> [--json]\n       floti desktop open history [<search>...] [--json]\n       floti desktop open settings [<section>] [--json]";

const DESKTOP: Family = Family {
    url_var: DESKTOP_URL_VAR,
    off: "This session has no desktop tools: they are off for it (Sessions can use Floti's window), or no Floti window was attached when it started.",
};

/// The tool `floti desktop open` calls.
const OPEN_TOOL: &str = "desktop_open";

/// A subcommand, as parsed: the tool it calls and the arguments it passes.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub tool: &'static str,
    pub arguments: Value,
    pub json: bool,
}

/// The subcommand in `args` (what follows `floti desktop`), with every
/// relative path made absolute against `here`, or why there is none.
///
/// # Errors
///
/// A sentence naming what is missing or not understood.
pub fn parse(args: &[String], here: &Path) -> Result<Command, String> {
    let Some((subcommand, rest)) = args.split_first() else {
        return Err("Say what to do: open.".to_owned());
    };
    match subcommand.as_str() {
        "open" => parse_open(rest, here),
        other => Err(format!("`{other}` is not a floti desktop command.")),
    }
}

/// `open <view> ...`.
fn parse_open(args: &[String], here: &Path) -> Result<Command, String> {
    let Some((view, rest)) = args.split_first() else {
        return Err(
            "Say which view to open: file, folder, project, history or settings.".to_owned(),
        );
    };
    let mut json = false;
    let mut line: Option<String> = None;
    let mut words: Vec<&str> = Vec::new();
    let mut flags_done = false;
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            word if flags_done => words.push(word),
            "--" => flags_done = true,
            "--json" => json = true,
            "--line" => {
                let value = rest
                    .next()
                    .ok_or_else(|| "`--line` takes the line to scroll to.".to_owned())?;
                line = Some(value.clone());
            }
            flag if flag.starts_with("--line=") => line = Some(flag["--line=".len()..].to_owned()),
            flag if flag.starts_with("--") => {
                return Err(format!("`{flag}` is not a floti desktop open option."));
            }
            word => words.push(word),
        }
    }
    let mut arguments = Map::new();
    arguments.insert("view".to_owned(), json!(view));
    if line.is_some() && view != "file" {
        return Err("`--line` goes only with a file.".to_owned());
    }
    match view.as_str() {
        "file" | "folder" | "project" => {
            let [path] = words.as_slice() else {
                return Err(format!("Name the one {view} to open."));
            };
            if path.trim().is_empty() {
                return Err(format!("Name the one {view} to open."));
            }
            arguments.insert("path".to_owned(), json!(absolute(path, here)));
            if let Some(line) = line {
                let line: u32 = line.parse().ok().filter(|line| *line >= 1).ok_or_else(|| {
                    format!("`--line {line}` is not a line number: lines count from 1.")
                })?;
                arguments.insert("line".to_owned(), json!(line));
            }
        }
        "history" => {
            arguments.insert("search".to_owned(), json!(words.join(" ")));
        }
        "settings" => match words.as_slice() {
            [] => {}
            [section] => {
                arguments.insert("section".to_owned(), json!(section));
            }
            _ => return Err("Name one Settings section, or none.".to_owned()),
        },
        other => {
            return Err(format!(
                "`{other}` is not a view floti desktop opens: file, folder, project, history or settings."
            ));
        }
    }
    Ok(Command {
        tool: OPEN_TOOL,
        arguments: Value::Object(arguments),
        json,
    })
}

/// `path` as the host should read it: as given when absolute, else under
/// `here`.
fn absolute(path: &str, here: &Path) -> String {
    let given = Path::new(path);
    if given.is_absolute() {
        path.to_owned()
    } else {
        here.join(given).display().to_string()
    }
}

/// Runs `command` against the family at `FLOTI_AGENT_DESKTOP_URL` with the
/// token in `FLOTI_AGENT_TOKEN`, printing its outcome. Returns the exit
/// status.
pub fn run(command: &Command) -> u8 {
    let outcome = match address_and_token(DESKTOP_URL_VAR) {
        Some((url, token)) => execute(command, &url, &token),
        None => Outcome::refused(format!(
            "floti desktop runs inside a Floti session that was given its desktop tools: {DESKTOP_URL_VAR} and {TOKEN_VAR} are not set here."
        )),
    };
    print_outcome(&outcome)
}

/// Calls the tool `command` stands for at `url`, as the session `token`
/// names: its sentence, or with `--json` the JSON it answered beside it.
pub fn execute(command: &Command, url: &str, token: &str) -> Outcome {
    match call(DESKTOP, url, token, command.tool, command.arguments.clone()) {
        Ok(answer) => {
            let first = answer.blocks.first().cloned().unwrap_or_default();
            if answer.failed {
                Outcome::refused(first)
            } else if command.json {
                Outcome::done(answer.blocks.get(1).cloned().unwrap_or(first))
            } else {
                Outcome::done(first)
            }
        }
        Err(problem) => Outcome::refused(problem),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::agent_cli::tests::{host, rpc_result, tool_answer};

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    fn here() -> PathBuf {
        std::env::temp_dir().join("a session folder")
    }

    fn open(arguments: Value, json: bool) -> Command {
        Command {
            tool: "desktop_open",
            arguments,
            json,
        }
    }

    #[test]
    fn open_takes_each_view_with_what_it_needs_and_makes_a_relative_path_absolute_here() {
        let src = here().join("src").join("main.rs").display().to_string();
        assert_eq!(
            parse(
                &args(&["open", "file", "src/main.rs", "--line", "42"]),
                &here()
            ),
            Ok(open(
                json!({ "view": "file", "path": here().join("src/main.rs").display().to_string(), "line": 42 }),
                false
            ))
        );
        let Ok(command) = parse(
            &args(&["open", "file", "--line=7", "--json", &src]),
            &here(),
        ) else {
            panic!("a file");
        };
        assert_eq!(
            command,
            open(json!({ "view": "file", "path": src, "line": 7 }), true)
        );
        let absolute = here().join("docs").display().to_string();
        assert_eq!(
            parse(&args(&["open", "folder", &absolute]), &here()),
            Ok(open(json!({ "view": "folder", "path": absolute }), false))
        );
        assert_eq!(
            parse(&args(&["open", "project", "."]), &here()),
            Ok(open(
                json!({ "view": "project", "path": here().join(".").display().to_string() }),
                false
            ))
        );
        assert_eq!(
            parse(&args(&["open", "history", "login", "bug"]), &here()),
            Ok(open(
                json!({ "view": "history", "search": "login bug" }),
                false
            ))
        );
        assert_eq!(
            parse(&args(&["open", "history"]), &here()),
            Ok(open(json!({ "view": "history", "search": "" }), false))
        );
        assert_eq!(
            parse(&args(&["open", "settings", "agents"]), &here()),
            Ok(open(
                json!({ "view": "settings", "section": "agents" }),
                false
            ))
        );
        assert_eq!(
            parse(&args(&["open", "settings"]), &here()),
            Ok(open(json!({ "view": "settings" }), false))
        );
        // What follows `--` is a word even where it starts with dashes.
        assert_eq!(
            parse(&args(&["open", "file", "--", "--odd name.txt"]), &here()),
            Ok(open(
                json!({ "view": "file", "path": here().join("--odd name.txt").display().to_string() }),
                false
            ))
        );
    }

    #[test]
    fn open_refuses_what_it_cannot_read_saying_why() {
        for (words, says) in [
            (&[][..], "Say what to do"),
            (&["close"][..], "not a floti desktop command"),
            (&["open"][..], "Say which view"),
            (&["open", "window"][..], "not a view"),
            (&["open", "file"][..], "Name the one file"),
            (&["open", "file", "a", "b"][..], "Name the one file"),
            (&["open", "file", "a", "--line"][..], "takes the line"),
            (
                &["open", "file", "a", "--line", "0"][..],
                "lines count from 1",
            ),
            (
                &["open", "file", "a", "--line", "ten"][..],
                "lines count from 1",
            ),
            (
                &["open", "folder", "a", "--line", "3"][..],
                "only with a file",
            ),
            (
                &["open", "file", "a", "--wait"][..],
                "not a floti desktop open option",
            ),
            (
                &["open", "settings", "agents", "browser"][..],
                "one Settings section",
            ),
        ] {
            let refused = parse(&args(words), &here()).unwrap_err();
            assert!(refused.contains(says), "{words:?}: {refused}");
        }
    }

    #[test]
    fn open_prints_the_tools_sentence_or_its_json_and_a_refusal_exits_1() {
        let opened = host(vec![
            rpc_result(json!({})),
            tool_answer(
                false,
                &["Opened main.rs beside your tab.", r#"{"opened":"file"}"#],
            ),
            rpc_result(json!({})),
            tool_answer(
                false,
                &["Opened main.rs beside your tab.", r#"{"opened":"file"}"#],
            ),
        ]);
        let command = open(json!({ "view": "file", "path": "/x/main.rs" }), false);
        assert_eq!(
            execute(&command, &opened.url, "secret-token"),
            Outcome::done("Opened main.rs beside your tab.".to_owned())
        );
        let as_json = open(json!({ "view": "file", "path": "/x/main.rs" }), true);
        assert_eq!(
            execute(&as_json, &opened.url, "secret-token"),
            Outcome::done(r#"{"opened":"file"}"#.to_owned())
        );
        let seen = opened.seen.lock().unwrap().clone();
        let call = seen.last().expect("the call");
        assert!(call.contains(r#""name":"desktop_open""#), "{call}");
        assert!(
            call.contains("Authorization: Bearer secret-token"),
            "{call}"
        );

        let refused = host(vec![
            rpc_result(json!({})),
            tool_answer(
                true,
                &["/x is not inside a folder Floti knows, so it will not open it."],
            ),
        ]);
        let outcome = execute(&command, &refused.url, "secret-token");
        assert_eq!(outcome.code, 1);
        assert!(
            outcome
                .err
                .unwrap()
                .contains("not inside a folder Floti knows")
        );

        // A launch without the desktop tools has no route for them.
        let off = host(vec![(404, String::new())]);
        let outcome = execute(&command, &off.url, "secret-token");
        assert_eq!(outcome.code, 1);
        let message = outcome.err.expect("a message");
        assert!(message.contains("no desktop tools"), "{message}");
        assert!(!message.contains("secret-token"), "{message}");
    }
}
