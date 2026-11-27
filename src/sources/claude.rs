//! Claude Code transcripts: `<projects>/<slug>/<session>.jsonl`.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::model::{ActivityKind, Event};

/// Claude's folder name for a working directory: every character outside
/// `[A-Za-z0-9]` becomes `-`.
pub fn slug_for(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The most recently modified `.jsonl` transcript for `cwd`, if any.
pub fn find_transcript(projects_dir: &Path, cwd: &Path) -> Option<PathBuf> {
    newest_jsonl(&projects_dir.join(slug_for(cwd)))
}

pub(crate) fn newest_jsonl(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|path| {
            let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// Turns Claude transcript lines into events. Keeps just enough state to
/// pair tool results with their calls and to sum token use.
#[derive(Debug)]
pub struct ClaudeParser {
    context_window: u64,
    /// tool_use id -> tool name, until its result arrives.
    pending: HashMap<String, String>,
    tokens_total: u64,
}

impl ClaudeParser {
    /// `context_window` is the model's context size in tokens.
    pub fn new(context_window: u64) -> ClaudeParser {
        ClaudeParser {
            context_window: context_window.max(1),
            pending: HashMap::new(),
            tokens_total: 0,
        }
    }

    /// Events for one transcript line. Unknown or invalid lines yield nothing.
    pub fn parse_line(&mut self, session_id: &str, line: &str) -> Vec<Event> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        let Some(content) = value.pointer("/message/content").and_then(Value::as_array) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        let activity = |kind, detail: String| Event::Activity {
            id: session_id.to_string(),
            kind,
            detail,
        };
        match value.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                let mut used_tool = false;
                for item in content {
                    if item.get("type").and_then(Value::as_str) != Some("tool_use") {
                        continue;
                    }
                    used_tool = true;
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        self.pending.insert(id.to_string(), name.to_string());
                    }
                    let input = item.get("input").unwrap_or(&Value::Null);
                    let (kind, detail) = classify(name, input);
                    events.push(activity(kind, detail));
                }
                if !used_tool {
                    events.push(activity(ActivityKind::Thinking, String::new()));
                }
                if let Some(usage) = value.pointer("/message/usage") {
                    events.push(self.usage(session_id, usage));
                }
            }
            Some("user") => {
                for item in content {
                    if item.get("type").and_then(Value::as_str) != Some("tool_result") {
                        continue;
                    }
                    let Some(name) = item
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .and_then(|id| self.pending.remove(id))
                    else {
                        continue;
                    };
                    let failed = item.get("is_error").and_then(Value::as_bool) == Some(true);
                    if failed {
                        events.push(activity(ActivityKind::CommandFailed, name));
                    } else if name == "Bash" {
                        events.push(activity(ActivityKind::CommandPassed, name));
                    }
                }
            }
            _ => {}
        }
        events
    }

    fn usage(&mut self, session_id: &str, usage: &Value) -> Event {
        let field = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        let input = field("input_tokens");
        self.tokens_total += input + field("output_tokens");
        let in_context =
            input + field("cache_read_input_tokens") + field("cache_creation_input_tokens");
        Event::Usage {
            id: session_id.to_string(),
            tokens_total: self.tokens_total,
            context_used: Some(in_context as f32 / self.context_window as f32),
        }
    }
}

/// Map a tool call to an activity and a short, content-free label.
fn classify(name: &str, input: &Value) -> (ActivityKind, String) {
    let file = || {
        input
            .get("file_path")
            .or_else(|| input.get("notebook_path"))
            .and_then(Value::as_str)
            .and_then(|path| Path::new(path).file_name())
            .map(|file| file.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.to_string())
    };
    match name {
        "Read" | "NotebookRead" => (ActivityKind::Reading, file()),
        "Edit" | "Write" | "NotebookEdit" | "MultiEdit" => (ActivityKind::Editing, file()),
        "Grep" | "Glob" | "ToolSearch" | "WebSearch" | "WebFetch" => {
            (ActivityKind::Searching, name.to_string())
        }
        "Bash" => (
            ActivityKind::RunningCommand,
            first_word(input.get("command")),
        ),
        "Agent" | "Task" => (ActivityKind::SubagentStarted, name.to_string()),
        other => (
            ActivityKind::Thinking,
            other.rsplit("__").next().unwrap_or(other).to_string(),
        ),
    }
}

/// The program name of a shell command: its first word, nothing more.
///
/// Leading `NAME=value` assignments (which often hold tokens) and wrapper
/// programs such as `env` or `sudo`, with their flags, are skipped. Anything
/// that does not look like a plain program name becomes `command`, so a label
/// can never carry a secret or an argument.
pub(crate) fn first_word(command: Option<&Value>) -> String {
    const WRAPPERS: [&str; 8] = [
        "env", "sudo", "time", "nohup", "nice", "exec", "command", "doas",
    ];
    /// Wrapper flags that take a value as the next word.
    const VALUE_FLAGS: [&str; 4] = ["-u", "-g", "-n", "-C"];
    const FALLBACK: &str = "command";

    let Some(command) = command.and_then(Value::as_str) else {
        return FALLBACK.to_string();
    };
    let mut words = command.split_whitespace().peekable();
    let mut in_wrapper = false;
    while let Some(word) = words.next() {
        let is_assignment = word.split_once('=').is_some_and(|(name, _)| {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if is_assignment {
            continue;
        }
        if in_wrapper && word.starts_with('-') {
            if VALUE_FLAGS.contains(&word) {
                words.next();
            }
            continue;
        }
        if WRAPPERS.contains(&word) {
            in_wrapper = true;
            continue;
        }
        let name = word.rsplit('/').next().unwrap_or(word);
        let plain = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'));
        return if plain {
            name.to_string()
        } else {
            FALLBACK.to_string()
        };
    }
    FALLBACK.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ActivityKind, Event};
    use std::path::{Path, PathBuf};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude")
            .join(name)
    }

    fn parse_all(name: &str) -> Vec<Event> {
        let text = std::fs::read_to_string(fixture(name)).expect("fixture");
        let mut parser = ClaudeParser::new(200_000);
        text.lines()
            .flat_map(|line| parser.parse_line("s", line))
            .collect()
    }

    fn act(kind: ActivityKind, detail: &str) -> Event {
        Event::Activity {
            id: "s".into(),
            kind,
            detail: detail.into(),
        }
    }

    #[test]
    fn slug_replaces_every_non_alphanumeric() {
        assert_eq!(
            slug_for(Path::new("/home/user/.cache/tmp")),
            "-home-user--cache-tmp"
        );
        assert_eq!(slug_for(Path::new("/a/b-c/d_e")), "-a-b-c-d-e");
    }

    #[test]
    fn find_transcript_picks_newest_jsonl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("-w-app");
        std::fs::create_dir_all(&project).expect("mkdir");
        let old = project.join("old.jsonl");
        let new = project.join("new.jsonl");
        std::fs::write(&old, "").expect("old");
        std::fs::write(&new, "").expect("new");
        std::fs::write(project.join("notes.txt"), "").expect("txt");
        let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .expect("open old")
            .set_modified(earlier)
            .expect("set mtime");
        assert_eq!(find_transcript(dir.path(), Path::new("/w/app")), Some(new));
        assert_eq!(find_transcript(dir.path(), Path::new("/w/other")), None);
    }

    #[test]
    fn basic_fixture_maps_to_expected_events() {
        let events = parse_all("basic.jsonl");
        assert_eq!(
            events,
            [
                act(ActivityKind::Reading, "main.rs"),
                act(ActivityKind::Editing, "lib.rs"),
                act(ActivityKind::RunningCommand, "cargo"),
                act(ActivityKind::CommandPassed, "Bash"),
                act(ActivityKind::Thinking, ""),
                Event::Usage {
                    id: "s".into(),
                    tokens_total: 150,
                    context_used: Some(0.005),
                },
            ]
        );
    }

    #[test]
    fn failed_bash_maps_to_command_failed() {
        let events = parse_all("failed_bash.jsonl");
        assert_eq!(
            events,
            [
                act(ActivityKind::RunningCommand, "cargo"),
                act(ActivityKind::CommandFailed, "Bash"),
            ]
        );
    }

    #[test]
    fn detail_never_contains_command_arguments() {
        let mut parser = ClaudeParser::new(200_000);
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"x","name":"Bash","input":{"command":"  cargo test --secret-flag token=abc"}}]}}"#;
        assert_eq!(
            parser.parse_line("s", line),
            [act(ActivityKind::RunningCommand, "cargo")]
        );
    }

    #[test]
    fn command_label_never_leaks_env_values_or_odd_words() {
        let label = |command: &str| first_word(Some(&serde_json::Value::String(command.into())));
        assert_eq!(label("GITHUB_TOKEN=ghp_secret gh pr create"), "gh");
        assert_eq!(label("A=1 B=2 cargo build"), "cargo");
        assert_eq!(label("env -u GITHUB_TOKEN gh pr view"), "gh");
        assert_eq!(label("env API_KEY=x node app.js"), "node");
        assert_eq!(label("sudo pacman -S foo"), "pacman");
        assert_eq!(label("time cargo test"), "cargo");
        assert_eq!(label("nohup ./serve.sh &"), "serve.sh");
        assert_eq!(label("/usr/bin/cargo check"), "cargo");
        assert_eq!(label("\"$(cat token)\" run"), "command");
        assert_eq!(label("$TOOL --flag"), "command");
        assert_eq!(label("TOKEN=abc"), "command");
        assert_eq!(label("   "), "command");
    }

    #[test]
    fn other_tools_map_by_family() {
        let mut parser = ClaudeParser::new(200_000);
        let call = |name: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"{name}","name":"{name}","input":{{}}}}]}}}}"#
            )
        };
        assert_eq!(
            parser.parse_line("s", &call("Grep")),
            [act(ActivityKind::Searching, "Grep")]
        );
        assert_eq!(
            parser.parse_line("s", &call("Agent")),
            [act(ActivityKind::SubagentStarted, "Agent")]
        );
        assert_eq!(
            parser.parse_line("s", &call("mcp__claude-in-chrome__navigate")),
            [act(ActivityKind::Thinking, "navigate")]
        );
    }

    #[test]
    fn non_bash_error_is_command_failed_with_tool_name() {
        let mut parser = ClaudeParser::new(200_000);
        parser.parse_line(
            "s",
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/a/b.rs"}}]}}"#,
        );
        assert_eq!(
            parser.parse_line(
                "s",
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"e1","is_error":true,"content":"no match"}]}}"#
            ),
            [act(ActivityKind::CommandFailed, "Edit")]
        );
    }

    #[test]
    fn invalid_and_unknown_lines_yield_nothing() {
        let mut parser = ClaudeParser::new(200_000);
        assert!(parser.parse_line("s", "not json").is_empty());
        assert!(parser.parse_line("s", "").is_empty());
        assert!(
            parser
                .parse_line("s", r#"{"type":"summary","summary":"x"}"#)
                .is_empty()
        );
        assert!(
            parser
                .parse_line("s", r#"{"type":"assistant","message":"odd"}"#)
                .is_empty()
        );
    }
}
