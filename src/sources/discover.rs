//! Agents running outside agent-deck: Claude and Codex transcripts that were
//! written to recently.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde_json::Value;

use crate::{
    Result,
    model::{Session, SessionStatus, Tool},
    sources::{
        agentdeck::SessionLister,
        claude::slug_for,
        claude_sessions::{read_records, status_from},
        codex::{RolloutIndex, recent_day_dirs},
        opencode::OpenCodeDb,
    },
};

/// Lists one session per recently written Claude or Codex transcript.
#[derive(Debug)]
pub struct Discovery {
    pub claude_projects: PathBuf,
    pub codex_sessions: PathBuf,
    /// Transcripts quiet for longer than this are not listed.
    pub active_within: Duration,
    /// Transcripts written within this are `Running`, older ones `Idle`.
    pub running_within: Duration,
    /// How often every Claude project folder is listed; between full scans
    /// only folders that recently held an active transcript are read.
    pub full_scan_every: Duration,
    /// Claude's session-record folder and the proc root, for live status
    /// and process ids of discovered Claude sessions.
    pub claude_sessions: Option<(PathBuf, PathBuf)>,
    /// OpenCode's database, when OpenCode sessions should be listed.
    pub opencode: Option<OpenCodeDb>,
    codex_index: RefCell<RolloutIndex>,
    /// Claude project folders that held an active transcript at the last
    /// full scan, and when that scan ran.
    active_dirs: RefCell<(Option<SystemTime>, Vec<PathBuf>)>,
    /// Each Claude transcript's working directory, once found.
    claude_cwd: RefCell<HashMap<PathBuf, PathBuf>>,
}

impl Discovery {
    pub fn new(claude_projects: PathBuf, codex_sessions: PathBuf, active_within: Duration) -> Self {
        Discovery {
            claude_projects,
            codex_sessions,
            active_within,
            running_within: Duration::from_secs(60),
            full_scan_every: Duration::from_secs(15),
            claude_sessions: None,
            opencode: None,
            codex_index: RefCell::new(RolloutIndex::default()),
            active_dirs: RefCell::new((None, Vec::new())),
            claude_cwd: RefCell::new(HashMap::new()),
        }
    }

    /// Sessions for transcripts modified within `active_within` of `now`.
    pub fn list_at(&self, now: SystemTime) -> Vec<Session> {
        let mut found: Vec<(SystemTime, Session)> = Vec::new();
        let records = self
            .claude_sessions
            .as_ref()
            .map(|(dir, proc_root)| read_records(dir, proc_root))
            .unwrap_or_default();
        for dir in self.claude_dirs(now) {
            let slug = dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            for (modified, file) in self.recent_jsonl(&dir, now, |_| true) {
                let (title, path) = match self.claude_cwd_of(&file) {
                    Some(cwd) => (folder_name(&cwd), cwd),
                    // No cwd yet: the slug's last part is the best guess.
                    None => (
                        slug.rsplit('-')
                            .find(|part| !part.is_empty())
                            .unwrap_or("claude")
                            .to_string(),
                        PathBuf::new(),
                    ),
                };
                let mut session = self.session(Tool::Claude, &file, modified, now, title, path);
                let stem = file.file_stem().map(|stem| stem.to_string_lossy());
                if let Some(record) = stem.and_then(|stem| records.get(stem.as_ref())) {
                    session.pid = Some(record.pid);
                    if let Some(status) = record.status.as_deref().and_then(status_from) {
                        session.status = status;
                    }
                }
                found.push((modified, session));
            }
        }
        for day in recent_day_dirs(&self.codex_sessions) {
            let rollouts = self.recent_jsonl(&day, now, |name| name.starts_with("rollout-"));
            for (modified, file) in rollouts {
                let cwd = self.codex_index.borrow_mut().cwd_of(&file);
                let title = cwd
                    .as_deref()
                    .map(folder_name)
                    .unwrap_or_else(|| "codex".to_string());
                let path = cwd.unwrap_or_default();
                found.push((
                    modified,
                    self.session(Tool::Codex, &file, modified, now, title, path),
                ));
            }
        }
        // OpenCode keeps no transcript file; its sessions come from its
        // database. A database error leaves them out (`check` reports it).
        if let Some(db) = &self.opencode
            && let Ok(sessions) = db.sessions(now, self.active_within)
        {
            found.extend(sessions.into_iter().map(|session| (now, session)));
        }
        // Stable order: oldest first, so new agents join at the end.
        found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
        found.into_iter().map(|(_, session)| session).collect()
    }

    /// The Claude project folders to read this poll: all of them on a full
    /// scan (every `full_scan_every`), otherwise only those that held an
    /// active transcript at the last full scan.
    fn claude_dirs(&self, now: SystemTime) -> Vec<PathBuf> {
        let mut state = self.active_dirs.borrow_mut();
        let due = match state.0 {
            Some(last) => now
                .duration_since(last)
                .map_or(true, |since| since >= self.full_scan_every),
            None => true,
        };
        if due {
            let active: Vec<PathBuf> = std::fs::read_dir(&self.claude_projects)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|dir| !self.recent_jsonl(dir, now, |_| true).is_empty())
                .collect();
            *state = (Some(now), active);
        }
        state.1.clone()
    }

    /// The working directory a Claude transcript records, cached once found.
    fn claude_cwd_of(&self, file: &Path) -> Option<PathBuf> {
        if let Some(cwd) = self.claude_cwd.borrow().get(file) {
            return Some(cwd.clone());
        }
        let cwd = transcript_cwd(file)?;
        self.claude_cwd
            .borrow_mut()
            .insert(file.to_path_buf(), cwd.clone());
        Some(cwd)
    }

    /// `.jsonl` files directly in `dir` modified within `active_within`.
    fn recent_jsonl(
        &self,
        dir: &Path,
        now: SystemTime,
        name_ok: impl Fn(&str) -> bool,
    ) -> Vec<(SystemTime, PathBuf)> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "jsonl")
                    && path
                        .file_name()
                        .is_some_and(|name| name_ok(&name.to_string_lossy()))
            })
            .filter_map(|path| {
                let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
                let age = now.duration_since(modified).unwrap_or_default();
                (age <= self.active_within).then_some((modified, path))
            })
            .collect()
    }

    fn session(
        &self,
        tool: Tool,
        file: &Path,
        modified: SystemTime,
        now: SystemTime,
        title: String,
        path: PathBuf,
    ) -> Session {
        let stem = file
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let age = now.duration_since(modified).unwrap_or_default();
        Session {
            id: format!("{}:{stem}", tool.name()),
            title,
            path,
            status: if age <= self.running_within {
                SessionStatus::Running
            } else {
                SessionStatus::Idle
            },
            tool,
            tmux_session: None,
            transcript: Some(file.to_path_buf()),
            pid: None,
        }
    }
}

/// The last component of `path`, for a lane title.
fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The first `cwd` field in a Claude transcript's opening lines (Claude
/// writes it on every message; the first few lines are bookkeeping).
fn transcript_cwd(file: &Path) -> Option<PathBuf> {
    const SEARCH_BYTES: u64 = 64 * 1024;
    let reader = BufReader::new(File::open(file).ok()?.take(SEARCH_BYTES));
    reader
        .lines()
        .map_while(std::result::Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .find_map(|value| value.get("cwd")?.as_str().map(PathBuf::from))
}

/// True when an agent-deck session already follows `found`'s transcript:
/// the same Claude project folder, or the same Codex working directory.
fn covered_by(deck: &[Session], found: &Session) -> bool {
    deck.iter()
        .any(|session| match (&session.tool, &found.tool) {
            (Tool::Claude, Tool::Claude) => {
                let folder = found
                    .transcript
                    .as_deref()
                    .and_then(Path::parent)
                    .and_then(Path::file_name);
                folder.is_some_and(|folder| folder.to_string_lossy() == slug_for(&session.path))
            }
            (Tool::Codex, Tool::Codex) | (Tool::OpenCode, Tool::OpenCode) => {
                session.path == found.path
            }
            _ => false,
        })
}

impl SessionLister for Discovery {
    fn list(&self) -> Result<Vec<Session>> {
        Ok(self.list_at(SystemTime::now()))
    }
}

/// agent-deck's sessions plus the discovered ones it does not already cover.
pub struct Combined {
    pub primary: Box<dyn SessionLister + Send>,
    pub discovery: Discovery,
    last_error: RefCell<Option<String>>,
    /// agent-deck's last good answer, used while it fails so its lanes do
    /// not vanish for a poll.
    last_good: RefCell<Vec<Session>>,
}

impl Combined {
    pub fn new(primary: Box<dyn SessionLister + Send>, discovery: Discovery) -> Self {
        Combined {
            primary,
            discovery,
            last_error: RefCell::new(None),
            last_good: RefCell::new(Vec::new()),
        }
    }
}

impl SessionLister for Combined {
    fn list(&self) -> Result<Vec<Session>> {
        let (mut sessions, error) = match self.primary.list() {
            Ok(sessions) => {
                *self.last_good.borrow_mut() = sessions.clone();
                (sessions, None)
            }
            Err(err) => (self.last_good.borrow().clone(), Some(err.to_string())),
        };
        *self.last_error.borrow_mut() = error;
        let discovered: Vec<Session> = self
            .discovery
            .list()?
            .into_iter()
            .filter(|found| !covered_by(&sessions, found))
            .collect();
        sessions.extend(discovered);
        Ok(sessions)
    }

    fn last_error(&self) -> Option<String> {
        self.last_error.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{SessionStatus, Tool},
        sources::agentdeck::Unavailable,
    };
    use std::path::Path;

    fn write(path: &Path, text: &str, seconds_ago: u64) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
        set_age(path, seconds_ago);
    }

    fn set_age(path: &Path, seconds_ago: u64) {
        let when = SystemTime::now() - Duration::from_secs(seconds_ago);
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open")
            .set_modified(when)
            .expect("mtime");
    }

    fn discovery(home: &Path) -> Discovery {
        Discovery::new(
            home.join(".claude/projects"),
            home.join(".codex/sessions"),
            Duration::from_secs(600),
        )
    }

    struct Fixed(Vec<Session>);
    impl SessionLister for Fixed {
        fn list(&self) -> Result<Vec<Session>> {
            Ok(self.0.clone())
        }
    }

    fn deck_session(id: &str, tool: Tool, path: &str) -> Session {
        Session {
            id: id.into(),
            title: id.into(),
            path: PathBuf::from(path),
            tool,
            status: SessionStatus::Running,
            tmux_session: None,
            transcript: None,
            pid: None,
        }
    }

    const META: &str = "{\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/w/api\"}}\n";

    #[test]
    fn recent_claude_transcript_becomes_a_session() {
        let home = tempfile::tempdir().expect("home");
        let file = home.path().join(".claude/projects/-w-my-app/abc.jsonl");
        write(&file, "{}\n", 5);
        let sessions = discovery(home.path()).list_at(SystemTime::now());
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.id, "claude:abc");
        assert_eq!(s.tool, Tool::Claude);
        assert_eq!(s.title, "app");
        assert_eq!(s.status, SessionStatus::Running);
        assert_eq!(s.transcript.as_deref(), Some(file.as_path()));
    }

    #[test]
    fn claude_title_and_path_come_from_the_transcript_cwd() {
        let home = tempfile::tempdir().expect("home");
        let file = home.path().join(".claude/projects/-w-my-app/abc.jsonl");
        write(
            &file,
            "{\"type\":\"mode\"}\n{\"type\":\"user\",\"cwd\":\"/w/my-app\"}\n",
            5,
        );
        let sessions = discovery(home.path()).list_at(SystemTime::now());
        assert_eq!(sessions[0].title, "my-app");
        assert_eq!(sessions[0].path, Path::new("/w/my-app"));
    }

    #[test]
    fn new_transcript_in_an_active_folder_shows_at_once() {
        let home = tempfile::tempdir().expect("home");
        let dir = home.path().join(".claude/projects/-w-app");
        write(&dir.join("one.jsonl"), "{}\n", 5);
        let discovery = discovery(home.path());
        let now = SystemTime::now();
        assert_eq!(discovery.list_at(now).len(), 1);
        write(&dir.join("two.jsonl"), "{}\n", 0);
        assert_eq!(discovery.list_at(now + Duration::from_secs(1)).len(), 2);
    }

    #[test]
    fn new_project_folder_is_found_on_the_next_full_scan() {
        let home = tempfile::tempdir().expect("home");
        let projects = home.path().join(".claude/projects");
        write(&projects.join("-w-app/one.jsonl"), "{}\n", 5);
        let discovery = discovery(home.path());
        let now = SystemTime::now();
        assert_eq!(discovery.list_at(now).len(), 1);
        write(&projects.join("-w-new/two.jsonl"), "{}\n", 0);
        // Between full scans only folders already known to be active are read.
        assert_eq!(discovery.list_at(now + Duration::from_secs(1)).len(), 1);
        let later = now + discovery.full_scan_every + Duration::from_secs(1);
        assert_eq!(discovery.list_at(later).len(), 2);
    }

    #[test]
    fn discovered_claude_session_takes_record_status_and_pid() {
        let home = tempfile::tempdir().expect("home");
        write(
            &home.path().join(".claude/projects/-w-app/abc.jsonl"),
            "{}\n",
            5,
        );
        let sessions_dir = home.path().join(".claude/sessions");
        std::fs::create_dir_all(&sessions_dir).expect("sessions");
        std::fs::write(
            sessions_dir.join("77.json"),
            "{\"pid\":77,\"sessionId\":\"abc\",\"status\":\"waiting\"}",
        )
        .expect("record");
        let proc_root = home.path().join("proc");
        std::fs::create_dir_all(proc_root.join("77")).expect("proc");
        let mut discovery = discovery(home.path());
        discovery.claude_sessions = Some((sessions_dir, proc_root));
        let sessions = discovery.list_at(SystemTime::now());
        assert_eq!(sessions[0].status, SessionStatus::Waiting);
        assert_eq!(sessions[0].pid, Some(77));
    }

    #[test]
    fn opencode_sessions_are_discovered_once() {
        use crate::sources::opencode::{OpenCodeDb, tests as oc};
        let home = tempfile::tempdir().expect("home");
        let path = home.path().join("opencode.db");
        let conn = oc::create(&path);
        let now = SystemTime::now();
        oc::add_session(&conn, "ses_a", None, "/w/site", oc::ms(now) - 1_000, true);
        oc::add_session(&conn, "ses_b", None, "/w/api", oc::ms(now) - 1_000, true);
        let mut discovery = discovery(home.path());
        discovery.opencode = Some(OpenCodeDb::new(path));
        let combined = Combined::new(
            Box::new(Fixed(vec![deck_session("deck", Tool::OpenCode, "/w/api")])),
            discovery,
        );
        let ids: Vec<String> = combined
            .list()
            .expect("list")
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, ["deck", "opencode:ses_a"]);
    }

    #[test]
    fn old_transcript_is_not_discovered() {
        let home = tempfile::tempdir().expect("home");
        write(
            &home.path().join(".claude/projects/-w-app/old.jsonl"),
            "{}\n",
            3600,
        );
        assert!(discovery(home.path()).list_at(SystemTime::now()).is_empty());
    }

    #[test]
    fn codex_rollout_session_uses_cwd_title() {
        let home = tempfile::tempdir().expect("home");
        let file = home
            .path()
            .join(".codex/sessions/2026/11/27/rollout-2026-11-27T10-00-00-x.jsonl");
        write(&file, META, 5);
        let sessions = discovery(home.path()).list_at(SystemTime::now());
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.id, "codex:rollout-2026-11-27T10-00-00-x");
        assert_eq!(s.tool, Tool::Codex);
        assert_eq!(s.title, "api");
        assert_eq!(s.path, Path::new("/w/api"));
        assert_eq!(s.transcript.as_deref(), Some(file.as_path()));
    }

    #[test]
    fn combined_drops_discovered_duplicate_of_agent_deck_session() {
        let home = tempfile::tempdir().expect("home");
        write(
            &home.path().join(".claude/projects/-w-app/one.jsonl"),
            "{}\n",
            5,
        );
        write(
            &home.path().join(".claude/projects/-w-other/two.jsonl"),
            "{}\n",
            5,
        );
        write(
            &home
                .path()
                .join(".codex/sessions/2026/11/27/rollout-2026-11-27T10-00-00-x.jsonl"),
            META,
            5,
        );
        let combined = Combined::new(
            Box::new(Fixed(vec![
                deck_session("a", Tool::Claude, "/w/app"),
                deck_session("c", Tool::Codex, "/w/api"),
            ])),
            discovery(home.path()),
        );
        let ids: Vec<String> = combined
            .list()
            .expect("list")
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, ["a", "c", "claude:two"]);
    }

    #[test]
    fn combined_keeps_discovered_sessions_when_agent_deck_fails() {
        let home = tempfile::tempdir().expect("home");
        write(
            &home.path().join(".claude/projects/-w-app/one.jsonl"),
            "{}\n",
            5,
        );
        let combined = Combined::new(
            Box::new(Unavailable("not found".into())),
            discovery(home.path()),
        );
        let sessions = combined.list().expect("discovered sessions still listed");
        assert_eq!(sessions.len(), 1);
        let error = combined.last_error().expect("error kept");
        assert!(error.contains("not found"), "{error}");
    }

    /// Lists `ok`, or fails while the shared flag is set.
    struct Flaky {
        ok: Vec<Session>,
        fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    impl SessionLister for Flaky {
        fn list(&self) -> Result<Vec<Session>> {
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                Err(crate::Error::AgentDeck("timed out".into()))
            } else {
                Ok(self.ok.clone())
            }
        }
    }

    #[test]
    fn agent_deck_failing_once_keeps_its_last_sessions() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let home = tempfile::tempdir().expect("home");
        write(
            &home.path().join(".claude/projects/-w-app/one.jsonl"),
            "{}\n",
            5,
        );
        let fail = Arc::new(AtomicBool::new(false));
        let flaky = Flaky {
            ok: vec![deck_session("a", Tool::Claude, "/w/app")],
            fail: Arc::clone(&fail),
        };
        let combined = Combined::new(Box::new(flaky), discovery(home.path()));
        let ids = |c: &Combined| -> Vec<String> {
            c.list().expect("list").into_iter().map(|s| s.id).collect()
        };
        assert_eq!(ids(&combined), ["a"]);
        fail.store(true, Ordering::SeqCst);
        assert_eq!(ids(&combined), ["a"], "same lane while agent-deck fails");
        assert!(combined.last_error().expect("error").contains("timed out"));
        fail.store(false, Ordering::SeqCst);
        assert_eq!(ids(&combined), ["a"]);
        assert_eq!(combined.last_error(), None);
    }

    #[test]
    fn discovered_session_goes_idle_then_disappears() {
        let home = tempfile::tempdir().expect("home");
        let file = home.path().join(".claude/projects/-w-app/one.jsonl");
        write(&file, "{}\n", 1);
        let mut discovery = discovery(home.path());
        discovery.active_within = Duration::from_secs(10);
        discovery.running_within = Duration::from_secs(3);
        let lister = Combined::new(Box::new(Fixed(Vec::new())), discovery);
        let mut collector =
            crate::collector::Collector::new(crate::config::Config::from_home(home.path()));
        let start = std::time::Instant::now();
        collector.step_at(&lister, start);
        assert_eq!(
            collector.world().lanes[0].session.status,
            SessionStatus::Running
        );
        set_age(&file, 5);
        collector.step_at(&lister, start + Duration::from_secs(4));
        assert_eq!(collector.world().lanes.len(), 1);
        assert_eq!(
            collector.world().lanes[0].session.status,
            SessionStatus::Idle
        );
        set_age(&file, 20);
        collector.step_at(&lister, start + Duration::from_secs(19));
        assert!(collector.world().lanes.is_empty());
    }
}
