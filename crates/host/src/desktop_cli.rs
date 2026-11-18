//! `floti desktop`: the floti-desktop family from a session's own shell.
//!
//! `floti desktop open <view> ...` opens one of Floti's views beside the
//! session's tab, as its `desktop_open` tool does: `file <path> [--line <n>]`,
//! `folder <path>`, `project <path>`, `history [<search>...]` and
//! `settings [<section>]`. `floti desktop settings` lists Floti's settings, as
//! `desktop_settings` does, and `floti desktop set <name> <value> ...` changes
//! one or several, as `desktop_set` does: a value is read as JSON where it is
//! JSON (`false`, `3`, `["C:\\work"]`, `null`) and as text otherwise
//! (`local`), and `--host <id>` names the host a per-host setting or a
//! plugin's switch changes on. `floti desktop status` says how Floti stands,
//! as `desktop_status` does, and `floti desktop log [<word>] [--lines <n>]`
//! answers the latest lines of the host's and the desktop's logs, as
//! `desktop_log` does. Each subcommand is one tool of the family, called
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
pub const USAGE: &str = "usage: floti desktop open file <path> [--line <n>] [--json]\n       floti desktop open folder <path> [--json]\n       floti desktop open project <path> [--json]\n       floti desktop open history [<search>...] [--json]\n       floti desktop open settings [<section>] [--json]\n       floti desktop settings [--json]\n       floti desktop set <name> <value> [<name> <value>...] [--host <id>] [--json]\n       floti desktop status [--json]\n       floti desktop log [<word>] [--lines <n>] [--json]";

const DESKTOP: Family = Family {
    url_var: DESKTOP_URL_VAR,
    off: "This session has no desktop tools: they are off for it (Sessions can use Floti's window), or no Floti window was attached when it started.",
};

/// The tool `floti desktop open` calls.
const OPEN_TOOL: &str = "desktop_open";
/// The tool `floti desktop settings` calls.
const SETTINGS_TOOL: &str = "desktop_settings";
/// The tool `floti desktop set` calls.
const SET_TOOL: &str = "desktop_set";
/// The tool `floti desktop status` calls.
const STATUS_TOOL: &str = "desktop_status";
/// The tool `floti desktop log` calls.
const LOG_TOOL: &str = "desktop_log";

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
        return Err("Say what to do: open, settings, set, status or log.".to_owned());
    };
    match subcommand.as_str() {
        "open" => parse_open(rest, here),
        "settings" => parse_settings(rest),
        "set" => parse_set(rest),
        "status" => parse_status(rest),
        "log" => parse_log(rest),
        other => Err(format!("`{other}` is not a floti desktop command.")),
    }
}

/// `settings`: nothing but `--json`.
fn parse_settings(args: &[String]) -> Result<Command, String> {
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            other => {
                return Err(format!(
                    "`{other}` is not something floti desktop settings takes: it lists every setting."
                ));
            }
        }
    }
    Ok(Command {
        tool: SETTINGS_TOOL,
        arguments: json!({}),
        json,
    })
}

/// `status`: nothing but `--json`.
fn parse_status(args: &[String]) -> Result<Command, String> {
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            other => {
                return Err(format!(
                    "`{other}` is not something floti desktop status takes: it says how Floti stands."
                ));
            }
        }
    }
    Ok(Command {
        tool: STATUS_TOOL,
        arguments: json!({}),
        json,
    })
}

/// `log [<word>] [--lines <n>]`.
fn parse_log(args: &[String]) -> Result<Command, String> {
    let mut json = false;
    let mut lines: Option<String> = None;
    let mut words: Vec<&str> = Vec::new();
    let mut flags_done = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            word if flags_done => words.push(word),
            "--" => flags_done = true,
            "--json" => json = true,
            "--lines" => {
                let value = rest
                    .next()
                    .ok_or_else(|| "`--lines` takes how many lines.".to_owned())?;
                lines = Some(value.clone());
            }
            flag if flag.starts_with("--lines=") => {
                lines = Some(flag["--lines=".len()..].to_owned());
            }
            flag if flag.starts_with("--") => {
                return Err(format!("`{flag}` is not a floti desktop log option."));
            }
            word => words.push(word),
        }
    }
    let mut arguments = Map::new();
    match words.as_slice() {
        [] => {}
        [word] if !word.trim().is_empty() => {
            arguments.insert("word".to_owned(), json!(word));
        }
        _ => return Err("Name one word to look for, or none.".to_owned()),
    }
    if let Some(lines) = lines {
        let count: u32 = lines
            .parse()
            .ok()
            .filter(|count| (1..=500).contains(count))
            .ok_or_else(|| format!("`--lines {lines}` is not a number of lines from 1 to 500."))?;
        arguments.insert("lines".to_owned(), json!(count));
    }
    Ok(Command {
        tool: LOG_TOOL,
        arguments: Value::Object(arguments),
        json,
    })
}

/// `set <name> <value> [<name> <value>...] [--host <id>]`.
fn parse_set(args: &[String]) -> Result<Command, String> {
    let mut json = false;
    let mut host: Option<String> = None;
    let mut words: Vec<&str> = Vec::new();
    let mut flags_done = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            word if flags_done => words.push(word),
            "--" => flags_done = true,
            "--json" => json = true,
            "--host" => {
                let value = rest
                    .next()
                    .ok_or_else(|| "`--host` takes the host's id.".to_owned())?;
                host = Some(value.clone());
            }
            flag if flag.starts_with("--host=") => host = Some(flag["--host=".len()..].to_owned()),
            flag if flag.starts_with("--") => {
                return Err(format!("`{flag}` is not a floti desktop set option."));
            }
            word => words.push(word),
        }
    }
    let (pairs, odd) = words.as_chunks::<2>();
    if pairs.is_empty() || !odd.is_empty() {
        return Err("Give each setting its new value: floti desktop set <name> <value> [<name> <value>...].".to_owned());
    }
    let mut changes = Map::new();
    for [name, value] in pairs {
        if changes.contains_key(*name) {
            return Err(format!(
                "`{name}` is named twice: give each setting one value."
            ));
        }
        changes.insert((*name).to_owned(), setting_value(value));
    }
    let mut arguments = Map::new();
    arguments.insert("changes".to_owned(), Value::Object(changes));
    if let Some(host) = host {
        if host.trim().is_empty() {
            return Err("`--host` takes the host's id.".to_owned());
        }
        arguments.insert("host".to_owned(), json!(host));
    }
    Ok(Command {
        tool: SET_TOOL,
        arguments: Value::Object(arguments),
        json,
    })
}

/// A value as typed: JSON where it reads as JSON, else the text itself, so
/// `false` and `3` are what they say and `local` or a path is a string.
fn setting_value(typed: &str) -> Value {
    serde_json::from_str(typed).unwrap_or_else(|_| Value::String(typed.to_owned()))
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
    fn settings_takes_only_json_and_set_takes_names_and_values_read_as_json_where_they_are() {
        assert_eq!(
            parse(&args(&["settings"]), &here()),
            Ok(Command {
                tool: "desktop_settings",
                arguments: json!({}),
                json: false
            })
        );
        assert_eq!(
            parse(&args(&["settings", "--json"]), &here()).map(|command| command.json),
            Ok(true)
        );
        let folder = here().display().to_string();
        let roots = json!([folder]).to_string();
        assert_eq!(
            parse(
                &args(&[
                    "set",
                    "sessionAgents",
                    "false",
                    "agentDepth",
                    "1",
                    "browserReach",
                    "local",
                    "accentColor",
                    "null",
                    "scanRoots",
                    &roots,
                    "--host",
                    "build-box",
                    "--json"
                ]),
                &here()
            ),
            Ok(Command {
                tool: "desktop_set",
                arguments: json!({
                    "changes": {
                        "sessionAgents": false,
                        "agentDepth": 1,
                        "browserReach": "local",
                        "accentColor": null,
                        "scanRoots": [folder],
                    },
                    "host": "build-box",
                }),
                json: true
            })
        );
        // A path that is not JSON is the text as typed; `--` keeps a value
        // that starts with dashes.
        assert_eq!(
            parse(
                &args(&["set", "--host=laptop", "--", "terminalFontFamily", "--odd"]),
                &here()
            ),
            Ok(Command {
                tool: "desktop_set",
                arguments: json!({
                    "changes": { "terminalFontFamily": "--odd" },
                    "host": "laptop",
                }),
                json: false
            })
        );
        assert_eq!(
            parse(
                &args(&["set", "terminalShell", "C:\\shells\\pwsh.exe"]),
                &here()
            )
            .map(|command| command.arguments),
            Ok(json!({ "changes": { "terminalShell": "C:\\shells\\pwsh.exe" } }))
        );
        for (words, says) in [
            (&["settings", "agents"][..], "lists every setting"),
            (&["set"][..], "Give each setting its new value"),
            (
                &["set", "sessionAgents"][..],
                "Give each setting its new value",
            ),
            (
                &["set", "agentDepth", "1", "agentDepth", "2"][..],
                "named twice",
            ),
            (&["set", "agentDepth", "1", "--host"][..], "takes the host"),
            (&["set", "agentDepth", "1", "--host="][..], "takes the host"),
            (
                &["set", "agentDepth", "1", "--wait"][..],
                "not a floti desktop set option",
            ),
        ] {
            let refused = parse(&args(words), &here()).unwrap_err();
            assert!(refused.contains(says), "{words:?}: {refused}");
        }
    }

    #[test]
    fn status_takes_only_json_and_log_takes_a_word_and_how_many_lines() {
        assert_eq!(
            parse(&args(&["status", "--json"]), &here()),
            Ok(Command {
                tool: "desktop_status",
                arguments: json!({}),
                json: true
            })
        );
        assert_eq!(
            parse(&args(&["log"]), &here()),
            Ok(Command {
                tool: "desktop_log",
                arguments: json!({}),
                json: false
            })
        );
        assert_eq!(
            parse(
                &args(&["log", "plugin", "--lines", "500", "--json"]),
                &here()
            ),
            Ok(Command {
                tool: "desktop_log",
                arguments: json!({ "word": "plugin", "lines": 500 }),
                json: true
            })
        );
        assert_eq!(
            parse(&args(&["log", "--lines=3", "--", "--odd"]), &here())
                .map(|command| command.arguments),
            Ok(json!({ "word": "--odd", "lines": 3 }))
        );
        for (words, says) in [
            (&["status", "now"][..], "says how Floti stands"),
            (&["log", "a", "b"][..], "one word"),
            (&["log", "--lines"][..], "takes how many"),
            (&["log", "--lines", "0"][..], "from 1 to 500"),
            (&["log", "--lines", "501"][..], "from 1 to 500"),
            (&["log", "--since"][..], "not a floti desktop log option"),
        ] {
            let refused = parse(&args(words), &here()).unwrap_err();
            assert!(refused.contains(says), "{words:?}: {refused}");
        }
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
