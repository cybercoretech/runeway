//! `--demo`: four scripted agents, so Runeway can be seen without any real
//! agent running. Deterministic: the world depends only on the tick.

use std::path::PathBuf;

use crate::model::{ActivityKind, Event, Session, SessionStatus, Tool, World};

/// Animation ticks per second.
pub const TICKS_PER_SECOND: u64 = 30;
/// The script repeats every minute.
pub const LOOP_TICKS: u64 = 60 * TICKS_PER_SECOND;

/// The demo world at `tick`.
pub fn world_at(tick: u64) -> World {
    let t = tick % LOOP_TICKS;
    let s = t / TICKS_PER_SECOND;
    let mut world = World::default();
    for (id, title, tool, script) in AGENTS {
        let (status, kind, detail) = script(s);
        world.apply(Event::Session(Session {
            id: id.to_string(),
            title: title.to_string(),
            path: PathBuf::from(format!("/demo/{title}")),
            tool: tool.clone(),
            status,
            tmux_session: None,
            transcript: None,
        }));
        world.apply(Event::Activity {
            id: id.to_string(),
            kind,
            detail,
        });
        world.apply(Event::Usage {
            id: id.to_string(),
            // Per second, not per tick, so the world only changes once a second.
            tokens_total: 2_000 + s * 270,
            context_used: Some(0.1 + 0.7 * s as f32 / 60.0),
        });
    }
    world
}

type Script = fn(u64) -> (SessionStatus, ActivityKind, String);

const AGENTS: [(&str, &str, Tool, Script); 4] = [
    ("demo:rune", "glyphsmith", Tool::Claude, rune_runner),
    ("demo:brass", "forge", Tool::Codex, automaton),
    ("demo:drone", "skyline", Tool::OpenCode, drone),
    ("demo:twin", "twins", Tool::Gemini, twin_spirit),
];

fn running(kind: ActivityKind, detail: &str) -> (SessionStatus, ActivityKind, String) {
    (SessionStatus::Running, kind, detail.to_string())
}

/// A file name that changes every two seconds, so each read drops a coin.
fn file(s: u64) -> String {
    const FILES: [&str; 6] = [
        "main.rs", "lib.rs", "lane.rs", "view.rs", "cli.rs", "rng.rs",
    ];
    FILES[(s / 2) as usize % FILES.len()].to_string()
}

/// Reads, edits, runs the tests and passes, thinks, reads again.
fn rune_runner(s: u64) -> (SessionStatus, ActivityKind, String) {
    match s {
        0..12 => running(ActivityKind::Reading, &file(s)),
        12..24 => running(ActivityKind::Editing, &file(s / 3)),
        24..30 => running(ActivityKind::RunningCommand, "cargo"),
        30..33 => running(ActivityKind::CommandPassed, "cargo"),
        33..45 => running(ActivityKind::Thinking, ""),
        _ => running(ActivityKind::Reading, &file(s)),
    }
}

/// Runs a command that fails, fixes it, and passes.
fn automaton(s: u64) -> (SessionStatus, ActivityKind, String) {
    match s {
        0..10 => running(ActivityKind::Thinking, ""),
        10..18 => running(ActivityKind::RunningCommand, "make"),
        18..21 => running(ActivityKind::CommandFailed, "make"),
        21..27 => running(ActivityKind::Editing, &file(s)),
        27..33 => running(ActivityKind::RunningCommand, "make"),
        33..36 => running(ActivityKind::CommandPassed, "make"),
        _ => running(ActivityKind::Searching, &file(s)),
    }
}

/// Searches, then waits at the gate for a permission answer for a while.
fn drone(s: u64) -> (SessionStatus, ActivityKind, String) {
    match s {
        0..8 => running(ActivityKind::Searching, "TODO"),
        8..35 => (
            SessionStatus::Waiting,
            ActivityKind::WaitingForPermission,
            String::new(),
        ),
        _ => running(ActivityKind::Editing, &file(s)),
    }
}

/// Edits, starts a helper, thinks, then finishes at the beacon.
fn twin_spirit(s: u64) -> (SessionStatus, ActivityKind, String) {
    match s {
        0..18 => running(ActivityKind::Editing, &file(s)),
        18..22 => running(ActivityKind::SubagentStarted, "helper"),
        22..40 => running(ActivityKind::Thinking, ""),
        _ => running(ActivityKind::Finished, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ActivityKind, SessionStatus, Tool};

    #[test]
    fn demo_world_has_four_agents_and_one_waiting_at_some_tick() {
        let world = world_at(0);
        assert_eq!(world.lanes.len(), 4);
        let tools: Vec<&Tool> = world.lanes.iter().map(|l| &l.session.tool).collect();
        assert_eq!(
            tools,
            [&Tool::Claude, &Tool::Codex, &Tool::OpenCode, &Tool::Gemini]
        );
        let waits = (0..LOOP_TICKS)
            .step_by(TICKS_PER_SECOND as usize)
            .any(|t| world_at(t).waiting_count() == 1);
        assert!(waits, "someone waits at the gate during the loop");
    }

    #[test]
    fn demo_plays_every_scripted_moment() {
        let mut seen = Vec::new();
        for t in (0..LOOP_TICKS).step_by(TICKS_PER_SECOND as usize) {
            for lane in world_at(t).lanes {
                if !seen.contains(&lane.activity) {
                    seen.push(lane.activity);
                }
            }
        }
        for kind in [
            ActivityKind::Reading,
            ActivityKind::Editing,
            ActivityKind::RunningCommand,
            ActivityKind::CommandFailed,
            ActivityKind::CommandPassed,
            ActivityKind::WaitingForPermission,
            ActivityKind::Finished,
        ] {
            assert!(seen.contains(&kind), "{kind:?} never shown");
        }
        assert!(
            world_at(0)
                .lanes
                .iter()
                .all(|l| l.session.status != SessionStatus::Error)
        );
    }

    #[test]
    fn demo_loops() {
        for t in [0, 7, 450, 1234] {
            assert_eq!(world_at(t), world_at(t + LOOP_TICKS));
        }
    }

    #[test]
    fn tokens_grow_during_the_loop() {
        let start: u64 = world_at(0).lanes.iter().map(|l| l.tokens_total).sum();
        let later: u64 = world_at(LOOP_TICKS / 2)
            .lanes
            .iter()
            .map(|l| l.tokens_total)
            .sum();
        assert!(later > start);
    }
}
