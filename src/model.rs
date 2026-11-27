//! The event model every source produces, and the world state the views read.

use std::path::PathBuf;

/// Which coding agent a session runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Codex,
    OpenCode,
    Gemini,
    Other(String),
}

impl Tool {
    /// Parse agent-deck's `tool` field.
    pub fn from_agent_deck(value: &str) -> Tool {
        match value {
            "claude" => Tool::Claude,
            "codex" => Tool::Codex,
            "opencode" => Tool::OpenCode,
            "gemini" => Tool::Gemini,
            other => Tool::Other(other.to_string()),
        }
    }

    /// Short lower-case name for display and JSON.
    pub fn name(&self) -> &str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::OpenCode => "opencode",
            Tool::Gemini => "gemini",
            Tool::Other(name) => name,
        }
    }
}

/// agent-deck's view of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Running,
    Busy,
    /// Waiting for the owner, usually a permission prompt.
    Waiting,
    Idle,
    Ready,
    Stopped,
    Error,
    Unknown(String),
}

impl SessionStatus {
    /// Parse agent-deck's `status` field.
    pub fn from_agent_deck(value: &str) -> SessionStatus {
        match value {
            "running" => SessionStatus::Running,
            "busy" => SessionStatus::Busy,
            "waiting" => SessionStatus::Waiting,
            "idle" => SessionStatus::Idle,
            "ready" => SessionStatus::Ready,
            "stopped" => SessionStatus::Stopped,
            "error" => SessionStatus::Error,
            other => SessionStatus::Unknown(other.to_string()),
        }
    }

    /// Short lower-case name for display and JSON.
    pub fn name(&self) -> &str {
        match self {
            SessionStatus::Running => "running",
            SessionStatus::Busy => "busy",
            SessionStatus::Waiting => "waiting",
            SessionStatus::Idle => "idle",
            SessionStatus::Ready => "ready",
            SessionStatus::Stopped => "stopped",
            SessionStatus::Error => "error",
            SessionStatus::Unknown(other) => other,
        }
    }
}

/// What an agent is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    Thinking,
    Reading,
    Searching,
    Editing,
    RunningCommand,
    CommandFailed,
    CommandPassed,
    WaitingForPermission,
    Idle,
    SubagentStarted,
    Finished,
    Errored,
}

impl ActivityKind {
    /// Present-tense verb for status lines ("editing lib.rs").
    pub fn verb(self) -> &'static str {
        match self {
            ActivityKind::Thinking => "thinking",
            ActivityKind::Reading => "reading",
            ActivityKind::Searching => "searching",
            ActivityKind::Editing => "editing",
            ActivityKind::RunningCommand => "running",
            ActivityKind::CommandFailed => "failed",
            ActivityKind::CommandPassed => "passed",
            ActivityKind::WaitingForPermission => "waiting for you",
            ActivityKind::Idle => "idle",
            ActivityKind::SubagentStarted => "started a helper",
            ActivityKind::Finished => "finished",
            ActivityKind::Errored => "error",
        }
    }
}

/// One agent session, as listed by agent-deck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    pub tool: Tool,
    pub status: SessionStatus,
    pub tmux_session: Option<String>,
    /// The transcript to follow, when the lister already knows it
    /// (discovered sessions). `None` lets the collector search for it.
    pub transcript: Option<PathBuf>,
    /// The agent's process, when a source knows it (Claude session records).
    pub pid: Option<u32>,
}

/// Everything a source can report. `detail` is a short label only, never file
/// contents, full command lines or prompts.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Session(Session),
    SessionGone {
        id: String,
    },
    Activity {
        id: String,
        kind: ActivityKind,
        detail: String,
    },
    Usage {
        id: String,
        tokens_total: u64,
        context_used: Option<f32>,
    },
}

/// The view model for one agent.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneState {
    pub session: Session,
    /// What to show: `WaitingForPermission` while agent-deck says `waiting`,
    /// otherwise the last activity seen in the transcript.
    pub activity: ActivityKind,
    pub detail: String,
    pub tokens_total: u64,
    pub context_used: Option<f32>,
    pub transcript: Option<PathBuf>,
    /// Event counter at the last change (no clock, so tests stay deterministic).
    pub last_change: u64,
    /// Last activity from the transcript, kept while waiting.
    last_activity: (ActivityKind, String),
}

impl LaneState {
    fn new(session: Session, tick: u64) -> LaneState {
        let mut lane = LaneState {
            session,
            activity: ActivityKind::Idle,
            detail: String::new(),
            tokens_total: 0,
            context_used: None,
            transcript: None,
            last_change: tick,
            last_activity: (ActivityKind::Idle, String::new()),
        };
        lane.refresh();
        lane
    }

    fn refresh(&mut self) {
        if self.session.status == SessionStatus::Waiting {
            self.activity = ActivityKind::WaitingForPermission;
            self.detail = String::new();
        } else {
            self.activity = self.last_activity.0;
            self.detail = self.last_activity.1.clone();
        }
    }
}

/// All lanes, in the order agents first appeared.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct World {
    pub lanes: Vec<LaneState>,
    tick: u64,
}

impl World {
    /// Fold one event into the world.
    pub fn apply(&mut self, event: Event) {
        self.tick += 1;
        let tick = self.tick;
        match event {
            Event::Session(session) => match self.lane_mut(&session.id) {
                Some(lane) => {
                    lane.session = session;
                    lane.refresh();
                    lane.last_change = tick;
                }
                None => self.lanes.push(LaneState::new(session, tick)),
            },
            Event::SessionGone { id } => self.lanes.retain(|lane| lane.session.id != id),
            Event::Activity { id, kind, detail } => {
                if let Some(lane) = self.lane_mut(&id) {
                    lane.last_activity = (kind, detail);
                    lane.refresh();
                    lane.last_change = tick;
                }
            }
            Event::Usage {
                id,
                tokens_total,
                context_used,
            } => {
                if let Some(lane) = self.lane_mut(&id) {
                    lane.tokens_total = tokens_total;
                    lane.context_used = context_used;
                    lane.last_change = tick;
                }
            }
        }
    }

    /// Record which transcript feeds a lane (shown in `status` and `check`).
    pub fn set_transcript(&mut self, id: &str, transcript: Option<PathBuf>) {
        if let Some(lane) = self.lane_mut(id) {
            lane.transcript = transcript;
        }
    }

    /// How many agents are waiting for the owner.
    pub fn waiting_count(&self) -> usize {
        self.lanes
            .iter()
            .filter(|lane| lane.session.status == SessionStatus::Waiting)
            .count()
    }

    fn lane_mut(&mut self, id: &str) -> Option<&mut LaneState> {
        self.lanes.iter_mut().find(|lane| lane.session.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn session(id: &str, status: SessionStatus) -> Session {
        Session {
            id: id.to_string(),
            title: format!("title-{id}"),
            path: PathBuf::from("/work/app"),
            tool: Tool::Claude,
            status,
            tmux_session: Some(format!("tmux-{id}")),
            transcript: None,
            pid: None,
        }
    }

    fn activity(id: &str, kind: ActivityKind, detail: &str) -> Event {
        Event::Activity {
            id: id.to_string(),
            kind,
            detail: detail.to_string(),
        }
    }

    #[test]
    fn session_event_adds_then_updates_lane() {
        let mut world = World::default();
        world.apply(Event::Session(session("a", SessionStatus::Running)));
        world.apply(Event::Session(session("a", SessionStatus::Idle)));
        assert_eq!(world.lanes.len(), 1);
        assert_eq!(world.lanes[0].session.status, SessionStatus::Idle);
    }

    #[test]
    fn lanes_keep_order_of_first_appearance() {
        let mut world = World::default();
        world.apply(Event::Session(session("b", SessionStatus::Running)));
        world.apply(Event::Session(session("a", SessionStatus::Running)));
        world.apply(Event::Session(session("b", SessionStatus::Idle)));
        let ids: Vec<&str> = world.lanes.iter().map(|l| l.session.id.as_str()).collect();
        assert_eq!(ids, ["b", "a"]);
    }

    #[test]
    fn session_gone_removes_lane() {
        let mut world = World::default();
        world.apply(Event::Session(session("a", SessionStatus::Running)));
        world.apply(Event::SessionGone { id: "a".into() });
        assert!(world.lanes.is_empty());
    }

    #[test]
    fn activity_for_unknown_session_is_ignored() {
        let mut world = World::default();
        world.apply(activity("ghost", ActivityKind::Editing, "lib.rs"));
        assert!(world.lanes.is_empty());
    }

    #[test]
    fn activity_updates_lane_and_counter() {
        let mut world = World::default();
        world.apply(Event::Session(session("a", SessionStatus::Running)));
        let before = world.lanes[0].last_change;
        world.apply(activity("a", ActivityKind::Editing, "lib.rs"));
        let lane = &world.lanes[0];
        assert_eq!(lane.activity, ActivityKind::Editing);
        assert_eq!(lane.detail, "lib.rs");
        assert!(lane.last_change > before);
    }

    #[test]
    fn waiting_status_sets_waiting_activity_and_running_restores_it() {
        let mut world = World::default();
        world.apply(Event::Session(session("a", SessionStatus::Running)));
        world.apply(activity("a", ActivityKind::Editing, "lib.rs"));
        world.apply(Event::Session(session("a", SessionStatus::Waiting)));
        assert_eq!(world.lanes[0].activity, ActivityKind::WaitingForPermission);
        world.apply(Event::Session(session("a", SessionStatus::Running)));
        assert_eq!(world.lanes[0].activity, ActivityKind::Editing);
        assert_eq!(world.lanes[0].detail, "lib.rs");
    }

    #[test]
    fn activity_while_waiting_is_remembered_but_waiting_stays_visible() {
        let mut world = World::default();
        world.apply(Event::Session(session("a", SessionStatus::Waiting)));
        world.apply(activity("a", ActivityKind::RunningCommand, "cargo"));
        assert_eq!(world.lanes[0].activity, ActivityKind::WaitingForPermission);
        world.apply(Event::Session(session("a", SessionStatus::Busy)));
        assert_eq!(world.lanes[0].activity, ActivityKind::RunningCommand);
    }

    #[test]
    fn usage_updates_tokens_and_context() {
        let mut world = World::default();
        world.apply(Event::Session(session("a", SessionStatus::Running)));
        world.apply(Event::Usage {
            id: "a".into(),
            tokens_total: 12_400,
            context_used: Some(0.31),
        });
        assert_eq!(world.lanes[0].tokens_total, 12_400);
        assert_eq!(world.lanes[0].context_used, Some(0.31));
    }

    #[test]
    fn waiting_count_counts_waiting_lanes() {
        let mut world = World::default();
        world.apply(Event::Session(session("a", SessionStatus::Waiting)));
        world.apply(Event::Session(session("b", SessionStatus::Running)));
        world.apply(Event::Session(session("c", SessionStatus::Waiting)));
        assert_eq!(world.waiting_count(), 2);
    }

    #[test]
    fn tool_and_status_parse_from_agent_deck_strings() {
        assert_eq!(Tool::from_agent_deck("claude"), Tool::Claude);
        assert_eq!(Tool::from_agent_deck("codex"), Tool::Codex);
        assert_eq!(Tool::from_agent_deck("opencode"), Tool::OpenCode);
        assert_eq!(Tool::from_agent_deck("gemini"), Tool::Gemini);
        assert_eq!(Tool::from_agent_deck("pi"), Tool::Other("pi".into()));
        assert_eq!(
            SessionStatus::from_agent_deck("running"),
            SessionStatus::Running
        );
        assert_eq!(SessionStatus::from_agent_deck("busy"), SessionStatus::Busy);
        assert_eq!(
            SessionStatus::from_agent_deck("waiting"),
            SessionStatus::Waiting
        );
        assert_eq!(SessionStatus::from_agent_deck("idle"), SessionStatus::Idle);
        assert_eq!(
            SessionStatus::from_agent_deck("ready"),
            SessionStatus::Ready
        );
        assert_eq!(
            SessionStatus::from_agent_deck("stopped"),
            SessionStatus::Stopped
        );
        assert_eq!(
            SessionStatus::from_agent_deck("error"),
            SessionStatus::Error
        );
        assert_eq!(
            SessionStatus::from_agent_deck("odd"),
            SessionStatus::Unknown("odd".into())
        );
    }
}
