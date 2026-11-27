//! Claude Code's running-session records: `<sessions>/<pid>.json`, one per
//! live `claude` process. Only `pid`, `sessionId` and `status` are read;
//! the `.key` files next to them are never opened.

use std::{collections::HashMap, path::Path};

use serde::Deserialize;

use crate::model::SessionStatus;

#[derive(Deserialize)]
struct Raw {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(default)]
    status: Option<String>,
    /// The process start time (`/proc/<pid>/stat` field 22), so a pid the
    /// kernel has handed to another process is not mistaken for Claude.
    #[serde(rename = "procStart", default)]
    proc_start: Option<String>,
}

/// Field 22 of `stat` (start time), read after the last `)` so the
/// process name cannot shift the fields.
fn start_time(proc_root: &Path, pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19).map(str::to_string)
}

/// The record's process still runs: it exists and, when the record says
/// when it started, started then.
fn alive(raw: &Raw, proc_root: &Path) -> bool {
    if !proc_root.join(raw.pid.to_string()).is_dir() {
        return false;
    }
    match &raw.proc_start {
        Some(start) => start_time(proc_root, raw.pid).as_deref() == Some(start.as_str()),
        None => true,
    }
}

/// What Runeway takes from one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeSessionRecord {
    pub pid: u32,
    pub session_id: String,
    pub status: Option<String>,
}

/// Records of live processes, keyed by session id (the transcript's stem).
pub fn read_records(dir: &Path, proc_root: &Path) -> HashMap<String, ClaudeSessionRecord> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        // Only `<pid>.json`; the `.key` files are never opened.
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| {
            let raw: Raw = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            alive(&raw, proc_root).then_some(raw)
        })
        .map(|raw| {
            let record = ClaudeSessionRecord {
                pid: raw.pid,
                session_id: raw.session_id,
                status: raw.status,
            };
            (record.session_id.clone(), record)
        })
        .collect()
}

/// Claude's own status word, when Runeway understands it.
pub fn status_from(record: &str) -> Option<SessionStatus> {
    match record {
        "busy" => Some(SessionStatus::Running),
        "idle" => Some(SessionStatus::Idle),
        other if other.contains("wait") || other.contains("permission") => {
            Some(SessionStatus::Waiting)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Fixture {
        _root: tempfile::TempDir,
        sessions: PathBuf,
        proc_root: PathBuf,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().expect("root");
        let sessions = root.path().join("sessions");
        let proc_root = root.path().join("proc");
        std::fs::create_dir_all(&sessions).expect("sessions");
        std::fs::create_dir_all(&proc_root).expect("proc");
        Fixture {
            sessions,
            proc_root,
            _root: root,
        }
    }

    /// A record started at `start`; when `alive`, a process with that pid
    /// exists, started at `start` too.
    fn record(f: &Fixture, pid: u32, session: &str, status: &str, alive: bool) {
        record_started(f, pid, session, status, alive.then_some(1000), 1000);
    }

    /// A record claiming start time `start`; the live process (if any)
    /// actually started at `proc_start`.
    fn record_started(
        f: &Fixture,
        pid: u32,
        session: &str,
        status: &str,
        proc_start: Option<u64>,
        start: u64,
    ) {
        std::fs::write(
            f.sessions.join(format!("{pid}.json")),
            format!(
                "{{\"pid\":{pid},\"sessionId\":\"{session}\",\"status\":\"{status}\",\"procStart\":\"{start}\",\"cwd\":\"/w\"}}"
            ),
        )
        .expect("record");
        if let Some(actual) = proc_start {
            let dir = f.proc_root.join(pid.to_string());
            std::fs::create_dir_all(&dir).expect("proc pid");
            // Fields 3–21 are filler; field 22 is the start time.
            let filler = vec!["0"; 18].join(" ");
            std::fs::write(
                dir.join("stat"),
                format!("{pid} (claude) S {filler} {actual} 0"),
            )
            .expect("stat");
        }
    }

    #[test]
    fn reused_pid_is_ignored() {
        let f = fixture();
        // The record's process started at 1000; pid 44 now belongs to a
        // process started at 5000.
        record_started(&f, 44, "abc", "busy", Some(5000), 1000);
        assert!(read_records(&f.sessions, &f.proc_root).is_empty());
    }

    #[test]
    fn records_map_session_ids_to_pids() {
        let f = fixture();
        record(&f, 41, "abc", "busy", true);
        let records = read_records(&f.sessions, &f.proc_root);
        let r = records.get("abc").expect("abc");
        assert_eq!(r.pid, 41);
        assert_eq!(r.status.as_deref(), Some("busy"));
    }

    #[test]
    fn stale_record_is_ignored() {
        let f = fixture();
        record(&f, 42, "gone", "busy", false);
        assert!(read_records(&f.sessions, &f.proc_root).is_empty());
    }

    #[test]
    fn key_files_are_never_read() {
        let f = fixture();
        record(&f, 43, "abc", "idle", true);
        std::fs::write(
            f.sessions.join("43.secret.key"),
            "{\"sessionId\":\"leak\",\"pid\":43}",
        )
        .expect("key");
        let records = read_records(&f.sessions, &f.proc_root);
        assert_eq!(records.len(), 1);
        assert!(!records.contains_key("leak"));
    }

    #[test]
    fn status_values_map() {
        assert_eq!(status_from("busy"), Some(SessionStatus::Running));
        assert_eq!(status_from("idle"), Some(SessionStatus::Idle));
        assert_eq!(
            status_from("waiting_for_permission"),
            Some(SessionStatus::Waiting)
        );
        assert_eq!(status_from("mystery"), None);
    }
}
