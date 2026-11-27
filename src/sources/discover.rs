//! Agents running outside agent-deck: Claude and Codex transcripts that were
//! written to recently.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use crate::{
    Result,
    model::{Session, SessionStatus, Tool},
    sources::{
        agentdeck::SessionLister,
        claude::slug_for,
        codex::{RolloutIndex, recent_day_dirs},
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
    codex_index: RefCell<RolloutIndex>,
}

impl Discovery {
    pub fn new(claude_projects: PathBuf, codex_sessions: PathBuf, active_within: Duration) -> Self {
        Discovery {
            claude_projects,
            codex_sessions,
            active_within,
            running_within: Duration::from_secs(60),
            codex_index: RefCell::new(RolloutIndex::default()),
        }
    }

    /// Sessions for transcripts modified within `active_within` of `now`.
    pub fn list_at(&self, now: SystemTime) -> Vec<Session> {
        let mut found: Vec<(SystemTime, Session)> = Vec::new();
        let claude_dirs = std::fs::read_dir(&self.claude_projects)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path());
        for dir in claude_dirs {
            let slug = dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            for (modified, file) in self.recent_jsonl(&dir, now, |_| true) {
                let title = slug
                    .rsplit('-')
                    .find(|part| !part.is_empty())
                    .unwrap_or("claude")
                    .to_string();
                found.push((
                    modified,
                    self.session(Tool::Claude, &file, modified, now, title, PathBuf::new()),
                ));
            }
        }
        for day in recent_day_dirs(&self.codex_sessions) {
            let rollouts = self.recent_jsonl(&day, now, |name| name.starts_with("rollout-"));
            for (modified, file) in rollouts {
                let cwd = self.codex_index.borrow_mut().cwd_of(&file);
                let title = cwd
                    .as_deref()
                    .and_then(Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "codex".to_string());
                let path = cwd.unwrap_or_default();
                found.push((
                    modified,
                    self.session(Tool::Codex, &file, modified, now, title, path),
                ));
            }
        }
        // Stable order: oldest first, so new agents join at the end.
        found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
        found.into_iter().map(|(_, session)| session).collect()
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
        }
    }
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
            (Tool::Codex, Tool::Codex) => session.path == found.path,
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
}

impl Combined {
    pub fn new(primary: Box<dyn SessionLister + Send>, discovery: Discovery) -> Self {
        Combined {
            primary,
            discovery,
            last_error: RefCell::new(None),
        }
    }
}

impl SessionLister for Combined {
    fn list(&self) -> Result<Vec<Session>> {
        let (mut sessions, error) = match self.primary.list() {
            Ok(sessions) => (sessions, None),
            Err(err) => (Vec::new(), Some(err.to_string())),
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
