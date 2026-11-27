//! Plain-text and JSON status output, shared by `runeway status` and the list view.

use serde::Serialize;

use crate::model::{LaneState, World};

/// One agent, ready to print.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusRow {
    pub title: String,
    pub tool: String,
    pub status: String,
    /// "editing lib.rs", "waiting for you", …
    pub action: String,
    pub tokens: u64,
    pub context_percent: Option<u8>,
    pub waiting: bool,
}

#[derive(Debug, Serialize)]
struct StatusJson<'a> {
    agents: &'a [StatusRow],
    waiting: usize,
    agent_deck_error: Option<&'a str>,
}

/// Rows for every lane, in lane order.
pub fn status_rows(world: &World) -> Vec<StatusRow> {
    world.lanes.iter().map(row).collect()
}

fn row(lane: &LaneState) -> StatusRow {
    let verb = lane.activity.verb();
    let action = if lane.detail.is_empty() {
        verb.to_string()
    } else {
        format!("{verb} {}", lane.detail)
    };
    StatusRow {
        title: lane.session.title.clone(),
        tool: lane.session.tool.name().to_string(),
        status: lane.session.status.name().to_string(),
        action,
        tokens: lane.tokens_total,
        context_percent: lane
            .context_used
            .map(|used| (used * 100.0).round().clamp(0.0, 100.0) as u8),
        waiting: lane.activity == crate::model::ActivityKind::WaitingForPermission,
    }
}

/// `999`, `12.4k`, `3.2M`.
pub fn abbreviate_tokens(tokens: u64) -> String {
    match tokens {
        0..=999 => tokens.to_string(),
        1_000..=999_999 => format!("{:.1}k", tokens as f64 / 1_000.0),
        _ => format!("{:.1}M", tokens as f64 / 1_000_000.0),
    }
}

/// One aligned line per agent, then `N agents, M waiting`. If the session
/// list could not be refreshed, the reason comes first.
pub fn render_text(world: &World, source_error: Option<&str>) -> String {
    let rows = status_rows(world);
    let mut out = String::new();
    if let Some(error) = source_error {
        out.push_str(&format!("agent-deck unavailable: {error}\n"));
    }
    let title_width = rows
        .iter()
        .map(|row| row.title.chars().count())
        .max()
        .unwrap_or(0)
        .clamp(4, 24);
    for row in &rows {
        let title: String = row.title.chars().take(title_width).collect();
        let context = row
            .context_percent
            .map(|percent| format!("{percent}%"))
            .unwrap_or_else(|| "-".to_string());
        let mark = if row.waiting { '!' } else { ' ' };
        out.push_str(&format!(
            "{mark} {title:<title_width$}  {:<8}  {:<8}  {:<30}  {:>6} tok  {context:>4}\n",
            row.tool,
            row.status,
            row.action,
            abbreviate_tokens(row.tokens),
        ));
    }
    out.push_str(&agents_summary(rows.len(), world.waiting_count()));
    out.push('\n');
    out
}

/// `1 agent, 0 waiting` / `3 agents, 1 waiting`.
pub fn agents_summary(agents: usize, waiting: usize) -> String {
    format!("{}, {waiting} waiting", count_noun(agents, "agent"))
}

/// `1 session`, `3 sessions`.
pub fn count_noun(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// `{ "agents": [...], "waiting": N, "agent_deck_error": null | "..." }`.
pub fn render_json(world: &World, source_error: Option<&str>) -> String {
    let rows = status_rows(world);
    let json = StatusJson {
        agents: &rows,
        waiting: world.waiting_count(),
        agent_deck_error: source_error,
    };
    // Serializing plain strings and numbers cannot fail.
    serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ActivityKind, Event, Session, SessionStatus, Tool, World};
    use std::path::PathBuf;

    fn world() -> World {
        let mut world = World::default();
        let session = |id: &str, tool, status| Session {
            id: id.into(),
            title: id.into(),
            path: PathBuf::from("/w"),
            tool,
            status,
            tmux_session: None,
            transcript: None,
        };
        world.apply(Event::Session(session(
            "app",
            Tool::Claude,
            SessionStatus::Running,
        )));
        world.apply(Event::Activity {
            id: "app".into(),
            kind: ActivityKind::Editing,
            detail: "lib.rs".into(),
        });
        world.apply(Event::Usage {
            id: "app".into(),
            tokens_total: 12_400,
            context_used: Some(0.314),
        });
        world.apply(Event::Session(session(
            "api",
            Tool::Codex,
            SessionStatus::Waiting,
        )));
        world
    }

    #[test]
    fn token_counts_are_abbreviated() {
        assert_eq!(abbreviate_tokens(0), "0");
        assert_eq!(abbreviate_tokens(999), "999");
        assert_eq!(abbreviate_tokens(12_400), "12.4k");
        assert_eq!(abbreviate_tokens(3_200_000), "3.2M");
    }

    #[test]
    fn rows_carry_action_tokens_context_and_waiting() {
        let rows = status_rows(&world());
        assert_eq!(rows[0].title, "app");
        assert_eq!(rows[0].tool, "claude");
        assert_eq!(rows[0].action, "editing lib.rs");
        assert_eq!(rows[0].tokens, 12_400);
        assert_eq!(rows[0].context_percent, Some(31));
        assert!(!rows[0].waiting);
        assert_eq!(rows[1].action, "waiting for you");
        assert!(rows[1].waiting);
        assert_eq!(rows[1].context_percent, None);
    }

    #[test]
    fn text_output_has_rows_and_summary() {
        let text = render_text(&world(), None);
        let lines: Vec<&str> = text.lines().collect();
        assert!(
            lines[0].contains("app")
                && lines[0].contains("editing lib.rs")
                && lines[0].contains("12.4k")
                && lines[0].contains("31%")
        );
        assert!(lines[1].contains("api") && lines[1].contains("waiting for you"));
        assert_eq!(lines.last().copied(), Some("2 agents, 1 waiting"));
    }

    #[test]
    fn text_output_leads_with_source_error() {
        let text = render_text(&World::default(), Some("not found"));
        assert_eq!(
            text.lines().next(),
            Some("agent-deck unavailable: not found")
        );
        assert_eq!(text.lines().last(), Some("0 agents, 0 waiting"));
    }

    #[test]
    fn one_agent_is_singular() {
        assert_eq!(agents_summary(1, 0), "1 agent, 0 waiting");
        assert_eq!(agents_summary(2, 1), "2 agents, 1 waiting");
        assert_eq!(count_noun(1, "session"), "1 session");
        assert_eq!(count_noun(3, "session"), "3 sessions");
    }

    #[test]
    fn json_output_is_valid_and_complete() {
        let json = render_json(&world(), Some("x"));
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["waiting"], 1);
        assert_eq!(value["agents"].as_array().map(Vec::len), Some(2));
        assert_eq!(value["agents"][0]["context_percent"], 31);
        assert_eq!(value["agent_deck_error"], "x");
    }
}
