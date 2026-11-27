//! OpenCode: sessions and tool calls from its SQLite database, opened
//! read-only. Only the `session_v2` and `session_message` tables are read.

use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::Value;

use crate::{
    Error, Result,
    model::{ActivityKind, Session, SessionStatus, Tool},
    sources::claude::first_word,
};

/// How long a query waits for OpenCode's own writes before giving up.
const BUSY_TIMEOUT: Duration = Duration::from_millis(200);

fn db_error(err: rusqlite::Error) -> Error {
    Error::OpenCode(err.to_string())
}

/// OpenCode's database file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeDb {
    pub path: PathBuf,
    /// A session OpenCode marks busy counts as running only if it was
    /// updated this recently (a crash leaves the busy mark behind).
    pub running_within: Duration,
}

impl OpenCodeDb {
    pub fn new(path: PathBuf) -> OpenCodeDb {
        OpenCodeDb {
            path,
            running_within: Duration::from_secs(60),
        }
    }

    /// Top-level, unarchived sessions updated within `active_within`.
    pub fn sessions(&self, now: SystemTime, active_within: Duration) -> Result<Vec<Session>> {
        let Some(conn) = self.open()? else {
            return Ok(Vec::new());
        };
        let since = millis(
            now.checked_sub(active_within)
                .unwrap_or(SystemTime::UNIX_EPOCH),
        );
        let mut query = conn
            .prepare(
                "SELECT id, directory, time_busy, time_idle, time_updated FROM session_v2
                 WHERE parent_id IS NULL AND time_archived IS NULL AND time_suspended IS NULL
                   AND time_updated >= ?1
                 ORDER BY time_updated, id",
            )
            .map_err(db_error)?;
        let rows = query
            .query_map(params![since], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(db_error)?;
        let mut sessions = Vec::new();
        for row in rows {
            let (id, directory, busy, idle, updated) = row.map_err(db_error)?;
            let path = PathBuf::from(&directory);
            let title = path.file_name().map_or(directory.clone(), |name| {
                name.to_string_lossy().into_owned()
            });
            let recent = millis(now).saturating_sub(updated)
                <= i64::try_from(self.running_within.as_millis()).unwrap_or(i64::MAX);
            let running = busy.unwrap_or(0) > idle.unwrap_or(0) && recent;
            sessions.push(Session {
                id: format!("opencode:{id}"),
                title,
                path,
                tool: Tool::OpenCode,
                status: if running {
                    SessionStatus::Running
                } else {
                    SessionStatus::Idle
                },
                tmux_session: None,
                transcript: None,
                pid: None,
            });
        }
        Ok(sessions)
    }

    /// The session's current activity (from its newest assistant message)
    /// and its token total.
    pub fn latest(&self, session_id: &str) -> Result<Option<(ActivityKind, String, u64)>> {
        match self.reader()? {
            Some(reader) => reader.latest(session_id),
            None => Ok(None),
        }
    }

    /// One read-only connection for several `latest` calls (one poll).
    pub fn reader(&self) -> Result<Option<Reader>> {
        Ok(self.open()?.map(|conn| Reader { conn }))
    }

    /// A read-only connection, or `None` when OpenCode is not installed.
    fn open(&self) -> Result<Option<Connection>> {
        if !self.path.is_file() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(db_error)?;
        Ok(Some(conn))
    }
}

/// An open, read-only view of OpenCode's database.
pub struct Reader {
    conn: Connection,
}

impl Reader {
    pub fn latest(&self, session_id: &str) -> Result<Option<(ActivityKind, String, u64)>> {
        let conn = &self.conn;
        let tokens: Option<i64> = conn
            .query_row(
                "SELECT tokens_input + tokens_output + tokens_reasoning FROM session_v2 WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        let Some(tokens) = tokens else {
            return Ok(None);
        };
        let data: Option<String> = conn
            .query_row(
                "SELECT data FROM session_message
                 WHERE session_id = ?1 AND type = 'assistant' ORDER BY seq DESC LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        let (kind, label) = data
            .and_then(|data| serde_json::from_str::<Value>(&data).ok())
            .and_then(|message| last_tool(&message))
            .unwrap_or((ActivityKind::Thinking, String::new()));
        Ok(Some((kind, label, u64::try_from(tokens).unwrap_or(0))))
    }
}

fn millis(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The activity and label of a message's last tool call. Only the tool's
/// name, status, a file name and a command's first word are read.
fn last_tool(message: &Value) -> Option<(ActivityKind, String)> {
    let part = message
        .get("content")?
        .as_array()?
        .iter()
        .rev()
        .find(|part| part.get("type").and_then(Value::as_str) == Some("tool"))?;
    let name = part.get("name").and_then(Value::as_str).unwrap_or_default();
    let status = part
        .pointer("/state/status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let input = part.pointer("/state/input");
    let file = || {
        input
            .and_then(|input| input.get("filePath"))
            .and_then(Value::as_str)
            .and_then(|path| path.rsplit('/').next())
            .unwrap_or_default()
            .to_string()
    };
    let command = || first_word(input.and_then(|input| input.get("command")));
    Some(match name {
        "read" => (ActivityKind::Reading, file()),
        "glob" | "grep" | "list" | "webfetch" | "websearch" => {
            (ActivityKind::Searching, String::new())
        }
        "edit" | "write" | "patch" => (ActivityKind::Editing, file()),
        "shell" | "bash" => match status {
            "running" | "pending" => (ActivityKind::RunningCommand, command()),
            "error" => (ActivityKind::CommandFailed, command()),
            _ => (ActivityKind::CommandPassed, command()),
        },
        "question" if status == "running" => (ActivityKind::WaitingForPermission, String::new()),
        _ => (ActivityKind::Thinking, String::new()),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{SessionStatus, Tool};
    use rusqlite::{Connection, params};
    use std::path::Path;

    pub(crate) fn ms(t: SystemTime) -> i64 {
        t.duration_since(SystemTime::UNIX_EPOCH)
            .expect("after epoch")
            .as_millis() as i64
    }

    pub(crate) fn create(path: &Path) -> Connection {
        let conn = Connection::open(path).expect("open");
        let schema = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode/schema.sql"),
        )
        .expect("schema");
        conn.execute_batch(&schema).expect("schema");
        conn
    }

    pub(crate) fn add_session(
        conn: &Connection,
        id: &str,
        parent: Option<&str>,
        dir: &str,
        updated: i64,
        busy: bool,
    ) {
        let (time_busy, time_idle) = if busy {
            (updated, updated - 10)
        } else {
            (updated - 10, updated)
        };
        conn.execute(
            "INSERT INTO session_v2 (id, parent_id, directory, time_updated, time_busy, time_idle, tokens_input, tokens_output, tokens_reasoning)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1000, 200, 34)",
            params![id, parent, dir, updated, time_busy, time_idle],
        )
        .expect("session");
    }

    pub(crate) fn add_tool(
        conn: &Connection,
        session: &str,
        seq: i64,
        name: &str,
        status: &str,
        input: &str,
    ) {
        let data = format!(
            "{{\"content\":[{{\"type\":\"text\",\"text\":\"secret words\"}},{{\"type\":\"tool\",\"id\":\"t{seq}\",\"name\":\"{name}\",\"state\":{{\"status\":\"{status}\",\"input\":{input}}}}}]}}"
        );
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, seq, data) VALUES (?1, ?2, 'assistant', ?3, ?4)",
            params![format!("m{seq}"), session, seq, data],
        )
        .expect("message");
    }

    fn db() -> (tempfile::TempDir, OpenCodeDb, Connection) {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("opencode.db");
        let conn = create(&path);
        (dir, OpenCodeDb::new(path), conn)
    }

    const WINDOW: Duration = Duration::from_secs(600);

    #[test]
    fn recent_top_level_session_is_listed() {
        let (_d, db, conn) = db();
        let now = SystemTime::now();
        add_session(&conn, "ses_1", None, "/w/site", ms(now) - 5_000, true);
        let sessions = db.sessions(now, WINDOW).expect("sessions");
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.id, "opencode:ses_1");
        assert_eq!(s.tool, Tool::OpenCode);
        assert_eq!(s.title, "site");
        assert_eq!(s.path, Path::new("/w/site"));
        assert_eq!(s.status, SessionStatus::Running);
    }

    #[test]
    fn idle_session_is_idle() {
        let (_d, db, conn) = db();
        let now = SystemTime::now();
        add_session(&conn, "ses_1", None, "/w/site", ms(now) - 5_000, false);
        assert_eq!(
            db.sessions(now, WINDOW).expect("s")[0].status,
            SessionStatus::Idle
        );
    }

    #[test]
    fn child_and_archived_sessions_are_skipped() {
        let (_d, db, conn) = db();
        let now = SystemTime::now();
        add_session(&conn, "ses_1", None, "/w/a", ms(now) - 5_000, true);
        add_session(&conn, "ses_2", Some("ses_1"), "/w/a", ms(now) - 5_000, true);
        add_session(&conn, "ses_3", None, "/w/b", ms(now) - 5_000, true);
        conn.execute(
            "UPDATE session_v2 SET time_archived = 1 WHERE id = 'ses_3'",
            [],
        )
        .expect("archive");
        let ids: Vec<String> = db
            .sessions(now, WINDOW)
            .expect("s")
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, ["opencode:ses_1"]);
    }

    #[test]
    fn suspended_session_is_skipped() {
        let (_d, db, conn) = db();
        let now = SystemTime::now();
        add_session(&conn, "ses_1", None, "/w/a", ms(now) - 5_000, true);
        conn.execute("UPDATE session_v2 SET time_suspended = 1", [])
            .expect("suspend");
        assert!(db.sessions(now, WINDOW).expect("s").is_empty());
    }

    #[test]
    fn busy_but_quiet_session_is_idle() {
        // Marked busy, but nothing written for 5 minutes (OpenCode crashed).
        let (_d, db, conn) = db();
        let now = SystemTime::now();
        add_session(&conn, "ses_1", None, "/w/a", ms(now) - 300_000, true);
        assert_eq!(
            db.sessions(now, WINDOW).expect("s")[0].status,
            SessionStatus::Idle
        );
    }

    #[test]
    fn one_reader_answers_several_sessions() {
        let (_d, db, conn) = db();
        let now = ms(SystemTime::now());
        add_session(&conn, "ses_1", None, "/w/a", now, true);
        add_session(&conn, "ses_2", None, "/w/b", now, true);
        add_tool(
            &conn,
            "ses_1",
            1,
            "edit",
            "running",
            r#"{"filePath":"/w/a/x.rs"}"#,
        );
        add_tool(
            &conn,
            "ses_2",
            2,
            "read",
            "completed",
            r#"{"filePath":"/w/b/y.rs"}"#,
        );
        let reader = db.reader().expect("open").expect("exists");
        let one = reader.latest("ses_1").expect("one").expect("some");
        let two = reader.latest("ses_2").expect("two").expect("some");
        assert_eq!((one.0, one.1.as_str()), (ActivityKind::Editing, "x.rs"));
        assert_eq!((two.0, two.1.as_str()), (ActivityKind::Reading, "y.rs"));
    }

    #[test]
    fn old_session_is_not_listed() {
        let (_d, db, conn) = db();
        let now = SystemTime::now();
        add_session(&conn, "ses_1", None, "/w/a", ms(now) - 3_600_000, false);
        assert!(db.sessions(now, WINDOW).expect("s").is_empty());
    }

    #[test]
    fn latest_maps_tool_parts() {
        let cases = [
            (
                "read",
                "completed",
                r#"{"filePath":"/w/a/src/main.rs"}"#,
                ActivityKind::Reading,
                "main.rs",
            ),
            (
                "grep",
                "completed",
                r#"{"pattern":"x"}"#,
                ActivityKind::Searching,
                "",
            ),
            (
                "glob",
                "running",
                r#"{"pattern":"*.rs"}"#,
                ActivityKind::Searching,
                "",
            ),
            (
                "webfetch",
                "running",
                r#"{"url":"https://x"}"#,
                ActivityKind::Searching,
                "",
            ),
            (
                "edit",
                "running",
                r#"{"filePath":"/w/a/lib.rs"}"#,
                ActivityKind::Editing,
                "lib.rs",
            ),
            (
                "write",
                "completed",
                r#"{"filePath":"/w/a/new.rs"}"#,
                ActivityKind::Editing,
                "new.rs",
            ),
            (
                "shell",
                "running",
                r#"{"command":"cargo test --all"}"#,
                ActivityKind::RunningCommand,
                "cargo",
            ),
            (
                "shell",
                "completed",
                r#"{"command":"cargo test"}"#,
                ActivityKind::CommandPassed,
                "cargo",
            ),
            (
                "shell",
                "error",
                r#"{"command":"make"}"#,
                ActivityKind::CommandFailed,
                "make",
            ),
            (
                "question",
                "running",
                r#"{}"#,
                ActivityKind::WaitingForPermission,
                "",
            ),
        ];
        for (name, status, input, kind, label) in cases {
            let (_d, db, conn) = db();
            add_session(&conn, "ses_1", None, "/w/a", ms(SystemTime::now()), true);
            add_tool(
                &conn,
                "ses_1",
                1,
                "read",
                "completed",
                r#"{"filePath":"/w/old.rs"}"#,
            );
            add_tool(&conn, "ses_1", 2, name, status, input);
            let (got_kind, got_label, tokens) = db.latest("ses_1").expect("latest").expect("some");
            assert_eq!(
                (got_kind, got_label.as_str()),
                (kind, label),
                "{name} {status}"
            );
            assert_eq!(tokens, 1_234);
        }
    }

    #[test]
    fn no_tool_part_is_thinking() {
        let (_d, db, conn) = db();
        add_session(&conn, "ses_1", None, "/w/a", ms(SystemTime::now()), true);
        let (kind, label, _) = db.latest("ses_1").expect("latest").expect("some");
        assert_eq!((kind, label.as_str()), (ActivityKind::Thinking, ""));
    }

    #[test]
    fn missing_db_lists_nothing() {
        let dir = tempfile::tempdir().expect("dir");
        let db = OpenCodeDb::new(dir.path().join("nope.db"));
        assert!(
            db.sessions(SystemTime::now(), WINDOW)
                .expect("ok")
                .is_empty()
        );
        assert!(
            !dir.path().join("nope.db").exists(),
            "must not create the file"
        );
    }

    #[test]
    fn wrong_schema_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("opencode.db");
        Connection::open(&path)
            .expect("open")
            .execute_batch("CREATE TABLE other (x int);")
            .expect("schema");
        let err = OpenCodeDb::new(path)
            .sessions(SystemTime::now(), WINDOW)
            .expect_err("schema");
        assert!(err.to_string().contains("opencode"), "{err}");
    }

    #[test]
    fn locked_db_does_not_hang() {
        let (_d, db, conn) = db();
        add_session(&conn, "ses_1", None, "/w/a", ms(SystemTime::now()), true);
        conn.execute_batch("BEGIN EXCLUSIVE; UPDATE session_v2 SET directory = '/w/b';")
            .expect("lock");
        let start = std::time::Instant::now();
        let _ = db.sessions(SystemTime::now(), WINDOW);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "{:?}",
            start.elapsed()
        );
    }
}
