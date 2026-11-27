//! Polls the sources and folds what they report into one `World`.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

use crate::{
    config::Config,
    model::{ActivityKind, Event, Session, Tool, World},
    sources::{
        agentdeck::SessionLister,
        claude::{self, ClaudeParser},
        codex::{CodexParser, RolloutIndex},
        opencode::OpenCodeDb,
        tail::Tail,
    },
};

/// Prefix of the lane ids discovery gives OpenCode sessions.
const OPENCODE_PREFIX: &str = "opencode:";

/// The parser that matches a transcript's agent.
#[derive(Debug)]
enum Parser {
    Claude(ClaudeParser),
    Codex(CodexParser),
}

impl Parser {
    fn parse_line(&mut self, session_id: &str, line: &str) -> Vec<Event> {
        match self {
            Parser::Claude(parser) => parser.parse_line(session_id, line),
            Parser::Codex(parser) => parser.parse_line(session_id, line),
        }
    }
}

/// One followed transcript.
#[derive(Debug)]
struct Follower {
    tail: Tail,
    parser: Parser,
    /// When the file last grew (or was opened).
    last_growth: Instant,
}

/// Owns the world state and the open transcripts.
#[derive(Debug)]
pub struct Collector {
    config: Config,
    world: World,
    followers: HashMap<String, Follower>,
    source_error: Option<String>,
    codex_index: RolloutIndex,
    opencode: OpenCodeDb,
    /// The last activity, label and tokens reported per OpenCode lane.
    opencode_seen: HashMap<String, (ActivityKind, String, u64)>,
}

impl Collector {
    pub fn new(config: Config) -> Collector {
        Collector {
            world: World::default(),
            followers: HashMap::new(),
            source_error: None,
            codex_index: RolloutIndex::default(),
            opencode: OpenCodeDb::new(config.opencode_db.clone()),
            opencode_seen: HashMap::new(),
            config,
        }
    }

    /// One poll: refresh the session list, follow matching transcripts, read
    /// their new lines, and fold everything into the world. Returns the
    /// events applied, in order.
    pub fn step(&mut self, lister: &dyn SessionLister) -> Vec<Event> {
        self.step_at(lister, Instant::now())
    }

    /// `step` with the current time passed in, so tests can move time.
    pub fn step_at(&mut self, lister: &dyn SessionLister, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        match lister.list() {
            Ok(sessions) => {
                self.source_error = lister.last_error();
                self.diff_sessions(sessions, &mut events);
            }
            // Keep the last known world; the views show the reason.
            Err(err) => self.source_error = Some(err.to_string()),
        }
        for event in &events {
            self.world.apply(event.clone());
        }
        let mut transcript_events = self.read_transcripts(now);
        transcript_events.extend(self.poll_opencode());
        for event in &transcript_events {
            self.world.apply(event.clone());
        }
        events.append(&mut transcript_events);
        events
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// Why the session list could not be refreshed, if it could not.
    pub fn source_error(&self) -> Option<&str> {
        self.source_error.as_deref()
    }

    fn diff_sessions(&mut self, sessions: Vec<Session>, events: &mut Vec<Event>) {
        let gone: Vec<String> = self
            .world
            .lanes
            .iter()
            .map(|lane| lane.session.id.clone())
            .filter(|id| !sessions.iter().any(|session| &session.id == id))
            .collect();
        for id in gone {
            self.followers.remove(&id);
            events.push(Event::SessionGone { id });
        }
        for session in sessions {
            let unchanged = self.world.lanes.iter().any(|lane| lane.session == session);
            if !unchanged {
                events.push(Event::Session(session));
            }
        }
    }

    /// Start following newly found transcripts, switch to newer ones, and
    /// read what was appended since the last step.
    ///
    /// No two lanes follow the same file. A lane keeps its file until that
    /// file has been quiet for `switch_after` and a newer one exists, so lanes
    /// sharing a folder never flip between transcripts.
    fn read_transcripts(&mut self, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        let sessions: Vec<Session> = self
            .world
            .lanes
            .iter()
            .map(|lane| lane.session.clone())
            .collect();
        let mut claimed: HashSet<PathBuf> = self
            .followers
            .values()
            .map(|follower| follower.tail.path().to_path_buf())
            .collect();
        for session in sessions {
            let current = self
                .followers
                .get(&session.id)
                .map(|follower| (follower.tail.path().to_path_buf(), follower.last_growth));
            let candidates = self.candidates(&session);
            let mtime_of = |path: &Path| {
                candidates
                    .iter()
                    .find(|(_, candidate)| candidate == path)
                    .map(|(modified, _)| *modified)
            };
            // The newest file no other lane follows.
            let pick = candidates
                .iter()
                .map(|(_, path)| path)
                .find(|path| {
                    !claimed.contains(*path)
                        || current.as_ref().is_some_and(|(own, _)| own == *path)
                })
                .cloned();
            let switch_to = match (&current, &pick) {
                (None, Some(pick)) => Some(pick.clone()),
                (Some((own, last_growth)), Some(pick)) if pick != own => {
                    let quiet =
                        now.saturating_duration_since(*last_growth) >= self.config.switch_after;
                    let newer = mtime_of(pick) >= mtime_of(own);
                    (quiet && newer).then(|| pick.clone())
                }
                _ => None,
            };
            if let Some(path) = switch_to {
                if let Some((own, _)) = &current {
                    claimed.remove(own);
                }
                self.followers.remove(&session.id);
                if let Some((mut follower, lines)) = self.open(&session, &path, now) {
                    for line in lines {
                        events.extend(follower.parser.parse_line(&session.id, &line));
                    }
                    claimed.insert(path);
                    self.followers.insert(session.id.clone(), follower);
                }
            }
            let path = self
                .followers
                .get(&session.id)
                .map(|follower| follower.tail.path().to_path_buf());
            self.world.set_transcript(&session.id, path);
            let Some(follower) = self.followers.get_mut(&session.id) else {
                continue;
            };
            match follower.tail.read_new() {
                Ok(lines) => {
                    if !lines.is_empty() {
                        follower.last_growth = now;
                    }
                    for line in lines {
                        events.extend(follower.parser.parse_line(&session.id, &line));
                    }
                }
                // The file went away; look for it again next step.
                Err(_) => {
                    if let Some(gone) = self.followers.remove(&session.id) {
                        claimed.remove(gone.tail.path());
                    }
                    self.world.set_transcript(&session.id, None);
                }
            }
        }
        events
    }

    /// OpenCode lanes: ask the database for each one's current activity and
    /// tokens, and report only what changed.
    fn poll_opencode(&mut self) -> Vec<Event> {
        let ids: Vec<String> = self
            .world
            .lanes
            .iter()
            .filter(|lane| lane.session.tool == Tool::OpenCode)
            .map(|lane| lane.session.id.clone())
            .filter(|id| id.starts_with(OPENCODE_PREFIX))
            .collect();
        self.opencode_seen.retain(|id, _| ids.contains(id));
        if ids.is_empty() {
            return Vec::new();
        }
        // One connection for every OpenCode lane this poll.
        let Ok(Some(reader)) = self.opencode.reader() else {
            return Vec::new();
        };
        let mut events = Vec::new();
        for id in ids {
            let Ok(Some((kind, detail, tokens))) = reader.latest(&id[OPENCODE_PREFIX.len()..])
            else {
                continue;
            };
            let seen = self.opencode_seen.get(&id);
            if seen.is_none_or(|(k, d, _)| (*k, d.as_str()) != (kind, detail.as_str())) {
                events.push(Event::Activity {
                    id: id.clone(),
                    kind,
                    detail: detail.clone(),
                });
            }
            if seen.is_none_or(|(_, _, t)| *t != tokens) {
                events.push(Event::Usage {
                    id: id.clone(),
                    tokens_total: tokens,
                    context_used: None,
                });
            }
            self.opencode_seen.insert(id, (kind, detail, tokens));
        }
        for event in &events {
            self.world.apply(event.clone());
        }
        events
    }

    /// Transcripts that could belong to `session`, newest first.
    fn candidates(&mut self, session: &Session) -> Vec<(SystemTime, PathBuf)> {
        if let Some(path) = &session.transcript {
            let modified = std::fs::metadata(path).and_then(|meta| meta.modified());
            return modified
                .map(|modified| vec![(modified, path.clone())])
                .unwrap_or_default();
        }
        match session.tool {
            Tool::Claude => claude::candidates(&self.config.claude_projects, &session.path),
            Tool::Codex => self
                .codex_index
                .candidates(&self.config.codex_sessions, &session.path),
            _ => Vec::new(),
        }
    }

    fn open(
        &self,
        session: &Session,
        path: &Path,
        now: Instant,
    ) -> Option<(Follower, Vec<String>)> {
        let parser = match session.tool {
            Tool::Claude => Parser::Claude(ClaudeParser::new(self.config.claude_context_window)),
            Tool::Codex => Parser::Codex(CodexParser),
            _ => return None,
        };
        let (tail, lines) = Tail::open_at_end(path, self.config.backfill_bytes).ok()?;
        Some((
            Follower {
                tail,
                parser,
                last_growth: now,
            },
            lines,
        ))
    }
}

/// What `runeway check` reports about each source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReport {
    /// Number of sessions, or why agent-deck could not be asked.
    pub agent_deck: Result<usize, String>,
    pub claude_dir: bool,
    pub codex_dir: bool,
    /// Recent OpenCode sessions, or why the database could not be read.
    pub opencode: Result<usize, String>,
}

/// Check every source once.
pub fn check(config: &Config, lister: Option<&dyn SessionLister>) -> SourceReport {
    let agent_deck = match lister {
        Some(lister) => lister
            .list()
            .map(|sessions| sessions.len())
            .map_err(|err| err.to_string()),
        None => Err("not found on PATH (give --agent-deck PATH)".to_string()),
    };
    SourceReport {
        agent_deck,
        claude_dir: config.claude_projects.is_dir(),
        codex_dir: config.codex_sessions.is_dir(),
        opencode: if config.opencode_db.is_file() {
            OpenCodeDb::new(config.opencode_db.clone())
                .sessions(SystemTime::now(), config.discover_within)
                .map(|sessions| sessions.len())
                .map_err(|err| err.to_string())
        } else {
            Err(format!("{} not found", config.opencode_db.display()))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Config,
        model::{ActivityKind, Event, Session, SessionStatus, Tool},
        sources::agentdeck::SessionLister,
    };
    use std::{
        cell::RefCell,
        io::Write,
        path::{Path, PathBuf},
    };

    /// A lister whose answer the test controls.
    struct FakeLister(RefCell<crate::Result<Vec<Session>>>);

    impl FakeLister {
        fn new(sessions: Vec<Session>) -> FakeLister {
            FakeLister(RefCell::new(Ok(sessions)))
        }
        fn set(&self, answer: crate::Result<Vec<Session>>) {
            *self.0.borrow_mut() = answer;
        }
    }

    impl SessionLister for FakeLister {
        fn list(&self) -> crate::Result<Vec<Session>> {
            match &*self.0.borrow() {
                Ok(sessions) => Ok(sessions.clone()),
                Err(err) => Err(crate::Error::AgentDeck(err.to_string())),
            }
        }
    }

    fn session(id: &str, tool: Tool, path: &str) -> Session {
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

    fn config(home: &Path) -> Config {
        Config::from_home(home)
    }

    fn append(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open");
        file.write_all(text.as_bytes()).expect("write");
    }

    const READ_LINE: &str = "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"r\",\"name\":\"Read\",\"input\":{\"file_path\":\"/w/app/a.rs\"}}]}}\n";
    const EDIT_LINE: &str = "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"e\",\"name\":\"Edit\",\"input\":{\"file_path\":\"/w/app/b.rs\"}}]}}\n";

    #[test]
    fn claude_session_gets_activity_from_its_transcript() {
        let home = tempfile::tempdir().expect("home");
        let transcript = home.path().join(".claude/projects/-w-app/x.jsonl");
        append(&transcript, READ_LINE);
        let lister = FakeLister::new(vec![session("a", Tool::Claude, "/w/app")]);
        let mut collector = Collector::new(config(home.path()));
        collector.step(&lister);
        let lane = &collector.world().lanes[0];
        assert_eq!(lane.activity, ActivityKind::Reading);
        assert_eq!(lane.detail, "a.rs");
        assert_eq!(lane.transcript.as_deref(), Some(transcript.as_path()));
    }

    #[test]
    fn appended_lines_are_picked_up_on_next_step() {
        let home = tempfile::tempdir().expect("home");
        let transcript = home.path().join(".claude/projects/-w-app/x.jsonl");
        append(&transcript, READ_LINE);
        let lister = FakeLister::new(vec![session("a", Tool::Claude, "/w/app")]);
        let mut collector = Collector::new(config(home.path()));
        collector.step(&lister);
        append(&transcript, EDIT_LINE);
        let events = collector.step(&lister);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Activity {
                kind: ActivityKind::Editing,
                ..
            }
        )));
        assert_eq!(collector.world().lanes[0].detail, "b.rs");
    }

    #[test]
    fn unchanged_session_emits_no_session_event() {
        let home = tempfile::tempdir().expect("home");
        let lister = FakeLister::new(vec![session("a", Tool::Other("pi".into()), "/w/x")]);
        let mut collector = Collector::new(config(home.path()));
        collector.step(&lister);
        assert!(collector.step(&lister).is_empty());
    }

    #[test]
    fn session_without_transcript_still_listed() {
        let home = tempfile::tempdir().expect("home");
        let lister = FakeLister::new(vec![session("g", Tool::Gemini, "/w/docs")]);
        let mut collector = Collector::new(config(home.path()));
        collector.step(&lister);
        assert_eq!(collector.world().lanes.len(), 1);
        assert_eq!(collector.world().lanes[0].transcript, None);
    }

    #[test]
    fn transcript_that_appears_later_is_picked_up() {
        let home = tempfile::tempdir().expect("home");
        let lister = FakeLister::new(vec![session("a", Tool::Claude, "/w/app")]);
        let mut collector = Collector::new(config(home.path()));
        collector.step(&lister);
        assert_eq!(collector.world().lanes[0].transcript, None);
        append(
            &home.path().join(".claude/projects/-w-app/x.jsonl"),
            READ_LINE,
        );
        collector.step(&lister);
        assert_eq!(collector.world().lanes[0].activity, ActivityKind::Reading);
    }

    #[test]
    fn codex_session_follows_its_rollout() {
        let home = tempfile::tempdir().expect("home");
        let rollout = home
            .path()
            .join(".codex/sessions/2026/11/27/rollout-2026-11-27T10-00-00-x.jsonl");
        append(
            &rollout,
            "{\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/w/api\"}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call\",\"name\":\"exec\",\"call_id\":\"1\",\"input\":\"x\"}}\n",
        );
        let lister = FakeLister::new(vec![session("c", Tool::Codex, "/w/api")]);
        let mut collector = Collector::new(config(home.path()));
        collector.step(&lister);
        assert_eq!(
            collector.world().lanes[0].activity,
            ActivityKind::RunningCommand
        );
    }

    #[test]
    fn lister_error_keeps_last_world_and_sets_source_error() {
        let home = tempfile::tempdir().expect("home");
        let lister = FakeLister::new(vec![session("a", Tool::Gemini, "/w/x")]);
        let mut collector = Collector::new(config(home.path()));
        collector.step(&lister);
        lister.set(Err(crate::Error::AgentDeck("boom".into())));
        collector.step(&lister);
        assert_eq!(collector.world().lanes.len(), 1);
        assert!(collector.source_error().expect("error").contains("boom"));
        lister.set(Ok(vec![session("a", Tool::Gemini, "/w/x")]));
        collector.step(&lister);
        assert_eq!(collector.source_error(), None);
    }

    #[test]
    fn removed_session_emits_session_gone() {
        let home = tempfile::tempdir().expect("home");
        let lister = FakeLister::new(vec![
            session("a", Tool::Gemini, "/w/x"),
            session("b", Tool::Gemini, "/w/y"),
        ]);
        let mut collector = Collector::new(config(home.path()));
        collector.step(&lister);
        lister.set(Ok(vec![session("b", Tool::Gemini, "/w/y")]));
        let events = collector.step(&lister);
        assert!(events.contains(&Event::SessionGone { id: "a".into() }));
        assert_eq!(collector.world().lanes.len(), 1);
    }

    #[test]
    fn check_reports_sources() {
        let home = tempfile::tempdir().expect("home");
        std::fs::create_dir_all(home.path().join(".claude/projects")).expect("mkdir");
        let lister = FakeLister::new(vec![session("a", Tool::Gemini, "/w/x")]);
        let report = check(&config(home.path()), Some(&lister));
        assert_eq!(report.agent_deck, Ok(1));
        assert!(report.claude_dir);
        assert!(!report.codex_dir);
        let none = check(&config(home.path()), None);
        assert!(none.agent_deck.is_err());
    }

    fn set_age(path: &Path, seconds_ago: u64) {
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds_ago);
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open")
            .set_modified(when)
            .expect("mtime");
    }

    #[test]
    fn two_sessions_in_one_folder_follow_different_transcripts() {
        let home = tempfile::tempdir().expect("home");
        let dir = home.path().join(".claude/projects/-w-app");
        append(&dir.join("one.jsonl"), READ_LINE);
        append(&dir.join("two.jsonl"), EDIT_LINE);
        set_age(&dir.join("one.jsonl"), 60);
        let lister = FakeLister::new(vec![
            session("a", Tool::Claude, "/w/app"),
            session("b", Tool::Claude, "/w/app"),
        ]);
        let mut collector = Collector::new(config(home.path()));
        let start = std::time::Instant::now();
        collector.step_at(&lister, start);
        let first: Vec<Option<PathBuf>> = collector
            .world()
            .lanes
            .iter()
            .map(|l| l.transcript.clone())
            .collect();
        assert!(first[0].is_some() && first[1].is_some());
        assert_ne!(first[0], first[1], "both lanes follow the same file");
        // Stable across steps: no flip-flopping between the two files.
        for second in 1..5 {
            collector.step_at(&lister, start + std::time::Duration::from_secs(second));
            let now: Vec<Option<PathBuf>> = collector
                .world()
                .lanes
                .iter()
                .map(|l| l.transcript.clone())
                .collect();
            assert_eq!(now, first);
        }
    }

    #[test]
    fn newer_transcript_replaces_current_only_after_it_goes_quiet() {
        let home = tempfile::tempdir().expect("home");
        let dir = home.path().join(".claude/projects/-w-app");
        let old = dir.join("old.jsonl");
        let new = dir.join("new.jsonl");
        append(&old, READ_LINE);
        set_age(&old, 60);
        let lister = FakeLister::new(vec![session("a", Tool::Claude, "/w/app")]);
        let mut config = config(home.path());
        config.switch_after = std::time::Duration::from_secs(30);
        let mut collector = Collector::new(config);
        let start = std::time::Instant::now();
        collector.step_at(&lister, start);
        assert_eq!(
            collector.world().lanes[0].transcript.as_deref(),
            Some(old.as_path())
        );

        // A newer file appears while the current one is still being written.
        append(&old, READ_LINE);
        append(&new, EDIT_LINE);
        // Filesystem timestamps can tie for files written microseconds apart;
        // in real use the new file is newer by seconds, so say so explicitly.
        set_age(&old, 10);
        collector.step_at(&lister, start + std::time::Duration::from_secs(5));
        assert_eq!(
            collector.world().lanes[0].transcript.as_deref(),
            Some(old.as_path())
        );

        // The current file stays quiet past the limit: switch.
        collector.step_at(&lister, start + std::time::Duration::from_secs(40));
        assert_eq!(
            collector.world().lanes[0].transcript.as_deref(),
            Some(new.as_path())
        );
    }

    #[test]
    fn collector_follows_the_transcript_hint() {
        let home = tempfile::tempdir().expect("home");
        let dir = home.path().join(".claude/projects/-w-app");
        let hinted = dir.join("hinted.jsonl");
        append(&hinted, READ_LINE);
        set_age(&hinted, 60);
        append(&dir.join("newer.jsonl"), EDIT_LINE);
        let mut hinted_session = session("claude:hinted", Tool::Claude, "");
        hinted_session.transcript = Some(hinted.clone());
        let lister = FakeLister::new(vec![hinted_session]);
        let mut collector = Collector::new(config(home.path()));
        collector.step(&lister);
        let lane = &collector.world().lanes[0];
        assert_eq!(lane.transcript.as_deref(), Some(hinted.as_path()));
        assert_eq!(lane.activity, ActivityKind::Reading);
    }

    #[test]
    fn collector_emits_opencode_activity_once() {
        use crate::sources::opencode::tests as oc;
        let home = tempfile::tempdir().expect("home");
        let mut config = config(home.path());
        config.opencode_db = home.path().join("opencode.db");
        let conn = oc::create(&config.opencode_db);
        oc::add_session(
            &conn,
            "ses_1",
            None,
            "/w/a",
            oc::ms(std::time::SystemTime::now()),
            true,
        );
        oc::add_tool(
            &conn,
            "ses_1",
            1,
            "edit",
            "running",
            r#"{"filePath":"/w/a/lib.rs"}"#,
        );
        let lister = FakeLister::new(vec![session("opencode:ses_1", Tool::OpenCode, "/w/a")]);
        let mut collector = Collector::new(config);
        collector.step(&lister);
        let lane = &collector.world().lanes[0];
        assert_eq!(
            (lane.activity, lane.detail.as_str()),
            (ActivityKind::Editing, "lib.rs")
        );
        assert_eq!(lane.tokens_total, 1_234);
        let again = collector.step(&lister);
        assert!(again.is_empty(), "nothing changed: {again:?}");
    }

    #[test]
    fn check_reports_opencode() {
        let home = tempfile::tempdir().expect("home");
        let mut config = config(home.path());
        config.opencode_db = home.path().join("opencode.db");
        let report = check(&config, None);
        assert!(report.opencode.is_err());
        let conn = crate::sources::opencode::tests::create(&config.opencode_db);
        crate::sources::opencode::tests::add_session(
            &conn,
            "ses_1",
            None,
            "/w/a",
            crate::sources::opencode::tests::ms(std::time::SystemTime::now()),
            true,
        );
        assert_eq!(check(&config, None).opencode, Ok(1));
    }

    #[test]
    fn config_defaults_from_home() {
        let config = Config::from_home(Path::new("/h"));
        assert_eq!(config.claude_projects, Path::new("/h/.claude/projects"));
        assert_eq!(config.codex_sessions, Path::new("/h/.codex/sessions"));
        assert_eq!(config.claude_context_window, 200_000);
        assert_eq!(config.backfill_bytes, 262_144);
        assert_eq!(config.discover_within, std::time::Duration::from_secs(600));
        assert_eq!(
            config.opencode_db,
            Path::new("/h/.local/share/opencode/opencode.db")
        );
    }
}
