//! Codex rollouts: `<sessions>/YYYY/MM/DD/rollout-*.jsonl`.

use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde_json::Value;

use crate::model::{ActivityKind, Event};

/// How many of the most recent day folders to search.
const RECENT_DAYS: usize = 7;
/// Codex runs commands through its `exec` tool, whose input is a script, so
/// the only safe and meaningful label is the tool name.
const EXEC: &str = "exec";

/// The most recently modified rollout whose `session_meta.cwd` is `cwd`,
/// searching only the most recent day folders.
pub fn find_transcript(sessions_dir: &Path, cwd: &Path) -> Option<PathBuf> {
    RolloutIndex::default()
        .candidates(sessions_dir, cwd)
        .into_iter()
        .next()
        .map(|(_, path)| path)
}

/// Remembers each rollout's working directory, so the first line of a
/// rollout (which also holds Codex's long base instructions) is read once,
/// not on every poll.
#[derive(Debug, Default)]
pub struct RolloutIndex {
    cwd_by_path: HashMap<PathBuf, Option<PathBuf>>,
}

impl RolloutIndex {
    /// Rollouts in the recent day folders whose working directory is `cwd`,
    /// newest first.
    pub fn candidates(&mut self, sessions_dir: &Path, cwd: &Path) -> Vec<(SystemTime, PathBuf)> {
        let mut found: Vec<(SystemTime, PathBuf)> = recent_day_dirs(sessions_dir)
            .into_iter()
            .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "jsonl")
                    && path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with("rollout-"))
            })
            .filter(|path| {
                self.cwd_by_path
                    .entry(path.clone())
                    .or_insert_with(|| rollout_cwd(path))
                    .as_deref()
                    == Some(cwd)
            })
            .filter_map(|path| {
                let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
                Some((modified, path))
            })
            .collect();
        found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        found
    }

    /// The working directory recorded in `path`'s first line (cached).
    pub fn cwd_of(&mut self, path: &Path) -> Option<PathBuf> {
        self.cwd_by_path
            .entry(path.to_path_buf())
            .or_insert_with(|| rollout_cwd(path))
            .clone()
    }

    /// How many rollouts have been indexed.
    pub fn len(&self) -> usize {
        self.cwd_by_path.len()
    }

    /// True when nothing has been indexed yet.
    pub fn is_empty(&self) -> bool {
        self.cwd_by_path.is_empty()
    }
}

/// `YYYY/MM/DD` folders, newest first, at most `RECENT_DAYS`.
pub(crate) fn recent_day_dirs(root: &Path) -> Vec<PathBuf> {
    let children = |dir: &Path| -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        dirs.sort();
        dirs.reverse();
        dirs
    };
    let mut days = Vec::new();
    for year in children(root) {
        for month in children(&year) {
            for day in children(&month) {
                days.push(day);
                if days.len() == RECENT_DAYS {
                    return days;
                }
            }
        }
    }
    days
}

/// The working directory recorded in a rollout's first line.
fn rollout_cwd(path: &Path) -> Option<PathBuf> {
    // Read at most this much looking for the end of the first line.
    const MAX_FIRST_LINE: u64 = 256 * 1024;
    let mut first = String::new();
    BufReader::new(File::open(path).ok()?.take(MAX_FIRST_LINE))
        .read_line(&mut first)
        .ok()?;
    if !first.ends_with('\n') {
        return None;
    }
    let value: Value = serde_json::from_str(&first).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    value
        .pointer("/payload/cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
}

/// Turns Codex rollout lines into events.
#[derive(Debug, Default)]
pub struct CodexParser;

impl CodexParser {
    /// Events for one rollout line. Unknown or invalid lines yield nothing.
    pub fn parse_line(&mut self, session_id: &str, line: &str) -> Vec<Event> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        let activity = |kind, detail: &str| Event::Activity {
            id: session_id.to_string(),
            kind,
            detail: detail.to_string(),
        };
        let payload_type = value.pointer("/payload/type").and_then(Value::as_str);
        match (value.get("type").and_then(Value::as_str), payload_type) {
            (Some("response_item"), Some("reasoning" | "message")) => {
                vec![activity(ActivityKind::Thinking, "")]
            }
            (Some("response_item"), Some("custom_tool_call")) => {
                vec![activity(ActivityKind::RunningCommand, EXEC)]
            }
            (Some("response_item"), Some("custom_tool_call_output")) => {
                let failed = value
                    .pointer("/payload/output")
                    .and_then(first_output_line)
                    .map(|line| {
                        let line = line.to_ascii_lowercase();
                        line.starts_with("script failed")
                            || line.starts_with("script error")
                            || line.starts_with("aborted")
                    })
                    .unwrap_or(false);
                let kind = if failed {
                    ActivityKind::CommandFailed
                } else {
                    ActivityKind::CommandPassed
                };
                vec![activity(kind, EXEC)]
            }
            (Some("event_msg"), Some("token_count")) => {
                let Some(info) = value.pointer("/payload/info") else {
                    return Vec::new();
                };
                let total = info
                    .pointer("/total_token_usage/total_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let last_input = info
                    .pointer("/last_token_usage/input_tokens")
                    .and_then(Value::as_u64);
                let window = info.get("model_context_window").and_then(Value::as_u64);
                let context_used = match (last_input, window) {
                    (Some(input), Some(window)) if window > 0 => Some(input as f32 / window as f32),
                    _ => None,
                };
                vec![Event::Usage {
                    id: session_id.to_string(),
                    tokens_total: total,
                    context_used,
                }]
            }
            _ => Vec::new(),
        }
    }
}

/// First line of a tool output: either a plain string, or the first text item.
fn first_output_line(output: &Value) -> Option<String> {
    let text = match output {
        Value::String(text) => text.as_str(),
        Value::Array(items) => items
            .iter()
            .find_map(|item| item.get("text").and_then(Value::as_str))?,
        _ => return None,
    };
    text.lines().next().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ActivityKind, Event};
    use std::path::{Path, PathBuf};

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex/sessions")
    }

    fn act(kind: ActivityKind, detail: &str) -> Event {
        Event::Activity {
            id: "s".into(),
            kind,
            detail: detail.into(),
        }
    }

    #[test]
    fn find_transcript_matches_cwd_and_picks_newest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let day = |d: &str| dir.path().join(format!("2026/11/{d}"));
        std::fs::create_dir_all(day("26")).expect("mkdir");
        std::fs::create_dir_all(day("27")).expect("mkdir");
        let meta = |cwd: &str| {
            format!("{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{cwd}\"}}}}\n")
        };
        let older = day("26").join("rollout-a.jsonl");
        let newer = day("27").join("rollout-b.jsonl");
        let other = day("27").join("rollout-c.jsonl");
        std::fs::write(&older, meta("/work/app")).expect("write");
        std::fs::write(&newer, meta("/work/app")).expect("write");
        std::fs::write(&other, meta("/work/other")).expect("write");
        let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(&older)
            .expect("open")
            .set_modified(earlier)
            .expect("mtime");
        assert_eq!(
            find_transcript(dir.path(), Path::new("/work/app")),
            Some(newer)
        );
        assert_eq!(find_transcript(dir.path(), Path::new("/work/none")), None);
    }

    #[test]
    fn find_transcript_works_on_fixture_tree() {
        let found = find_transcript(&fixtures(), Path::new("/work/app")).expect("found");
        assert!(found.ends_with("rollout-2026-11-27T10-00-00-test.jsonl"));
    }

    #[test]
    fn fixture_maps_to_expected_events() {
        let path = fixtures().join("2026/11/27/rollout-2026-11-27T10-00-00-test.jsonl");
        let text = std::fs::read_to_string(path).expect("fixture");
        let mut parser = CodexParser;
        let events: Vec<Event> = text
            .lines()
            .flat_map(|line| parser.parse_line("s", line))
            .collect();
        assert_eq!(
            events,
            [
                act(ActivityKind::Thinking, ""),
                act(ActivityKind::RunningCommand, "exec"),
                act(ActivityKind::CommandPassed, "exec"),
                act(ActivityKind::RunningCommand, "exec"),
                act(ActivityKind::CommandFailed, "exec"),
                Event::Usage {
                    id: "s".into(),
                    tokens_total: 3200,
                    context_used: Some(0.1),
                },
            ]
        );
    }

    #[test]
    fn script_error_and_abort_count_as_failed() {
        let mut parser = CodexParser;
        let out = |text: &str| {
            format!(
                r#"{{"type":"response_item","payload":{{"type":"custom_tool_call_output","call_id":"x","output":[{{"type":"input_text","text":"{text}"}}]}}}}"#
            )
        };
        assert_eq!(
            parser.parse_line("s", &out("Script error: boom")),
            [act(ActivityKind::CommandFailed, "exec")]
        );
        assert_eq!(
            parser.parse_line(
                "s",
                r#"{"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"y","output":"aborted by user after 3.2s"}}"#
            ),
            [act(ActivityKind::CommandFailed, "exec")]
        );
    }

    #[test]
    fn rollout_index_reads_each_first_line_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let day = dir.path().join("2026/11/27");
        std::fs::create_dir_all(&day).expect("mkdir");
        let rollout = day.join("rollout-a.jsonl");
        std::fs::write(
            &rollout,
            "{\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/work/app\"}}\n",
        )
        .expect("write");
        let mut index = RolloutIndex::default();
        assert_eq!(
            index.candidates(dir.path(), Path::new("/work/app")).len(),
            1
        );
        // The first line is cached: rewriting it does not change the answer.
        std::fs::write(
            &rollout,
            "{\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/work/other\"}}\n",
        )
        .expect("write");
        assert_eq!(
            index.candidates(dir.path(), Path::new("/work/app")).len(),
            1
        );
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn rollout_with_oversized_first_line_is_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let day = dir.path().join("2026/11/27");
        std::fs::create_dir_all(&day).expect("mkdir");
        let huge = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"/work/app\",\"pad\":\"{}\"}}}}\n",
            "x".repeat(300 * 1024)
        );
        std::fs::write(day.join("rollout-big.jsonl"), huge).expect("write");
        let mut index = RolloutIndex::default();
        assert!(
            index
                .candidates(dir.path(), Path::new("/work/app"))
                .is_empty()
        );
    }

    #[test]
    fn unknown_lines_yield_nothing() {
        let mut parser = CodexParser;
        assert!(parser.parse_line("s", "garbage").is_empty());
        assert!(
            parser
                .parse_line(
                    "s",
                    r#"{"type":"event_msg","payload":{"type":"item_completed"}}"#
                )
                .is_empty()
        );
        assert!(
            parser
                .parse_line("s", r#"{"type":"session_meta","payload":{"cwd":"/x"}}"#)
                .is_empty()
        );
    }
}
