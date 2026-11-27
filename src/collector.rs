//! Polls the sources and folds what they report into one `World`.

use std::{collections::HashMap, path::PathBuf};

use crate::{
    config::Config,
    model::{Event, Session, Tool, World},
    sources::{
        agentdeck::SessionLister,
        claude::{self, ClaudeParser},
        codex::{self, CodexParser},
        tail::Tail,
    },
};

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
}

/// Owns the world state and the open transcripts.
#[derive(Debug)]
pub struct Collector {
    config: Config,
    world: World,
    followers: HashMap<String, Follower>,
    source_error: Option<String>,
}

impl Collector {
    pub fn new(config: Config) -> Collector {
        Collector {
            config,
            world: World::default(),
            followers: HashMap::new(),
            source_error: None,
        }
    }

    /// One poll: refresh the session list, follow matching transcripts, read
    /// their new lines, and fold everything into the world. Returns the
    /// events applied, in order.
    pub fn step(&mut self, lister: &dyn SessionLister) -> Vec<Event> {
        let mut events = Vec::new();
        match lister.list() {
            Ok(sessions) => {
                self.source_error = None;
                self.diff_sessions(sessions, &mut events);
            }
            // Keep the last known world; the views show the reason.
            Err(err) => self.source_error = Some(err.to_string()),
        }
        for event in &events {
            self.world.apply(event.clone());
        }
        let mut transcript_events = self.read_transcripts();
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
    fn read_transcripts(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let sessions: Vec<Session> = self
            .world
            .lanes
            .iter()
            .map(|lane| lane.session.clone())
            .collect();
        for session in sessions {
            let found = self.find_transcript(&session);
            let current = self
                .followers
                .get(&session.id)
                .map(|follower| follower.tail.path().to_path_buf());
            if found.is_some() && found != current {
                self.followers.remove(&session.id);
                if let Some((follower, lines)) =
                    found.as_ref().and_then(|path| self.open(&session, path))
                {
                    let mut follower = follower;
                    for line in lines {
                        events.extend(follower.parser.parse_line(&session.id, &line));
                    }
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
                    for line in lines {
                        events.extend(follower.parser.parse_line(&session.id, &line));
                    }
                }
                // The file went away; look for it again next step.
                Err(_) => {
                    self.followers.remove(&session.id);
                    self.world.set_transcript(&session.id, None);
                }
            }
        }
        events
    }

    fn find_transcript(&self, session: &Session) -> Option<PathBuf> {
        match session.tool {
            Tool::Claude => claude::find_transcript(&self.config.claude_projects, &session.path),
            Tool::Codex => codex::find_transcript(&self.config.codex_sessions, &session.path),
            _ => None,
        }
    }

    fn open(&self, session: &Session, path: &std::path::Path) -> Option<(Follower, Vec<String>)> {
        let parser = match session.tool {
            Tool::Claude => Parser::Claude(ClaudeParser::new(self.config.claude_context_window)),
            Tool::Codex => Parser::Codex(CodexParser),
            _ => return None,
        };
        let (tail, lines) = Tail::open_at_end(path, self.config.backfill_bytes).ok()?;
        Some((Follower { tail, parser }, lines))
    }
}

/// What `runeway check` reports about each source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReport {
    /// Number of sessions, or why agent-deck could not be asked.
    pub agent_deck: Result<usize, String>,
    pub claude_dir: bool,
    pub codex_dir: bool,
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

    #[test]
    fn config_defaults_from_home() {
        let config = Config::from_home(Path::new("/h"));
        assert_eq!(config.claude_projects, Path::new("/h/.claude/projects"));
        assert_eq!(config.codex_sessions, Path::new("/h/.codex/sessions"));
        assert_eq!(config.claude_context_window, 200_000);
        assert_eq!(config.backfill_bytes, 262_144);
    }
}
