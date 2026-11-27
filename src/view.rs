//! The list view: key handling and drawing. Pure apart from the `Frame`, so
//! it is tested with ratatui's `TestBackend`, never a real terminal.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table},
};
use runeway::{
    model::World,
    status::{abbreviate_tokens, count_noun, status_rows},
};

const GOLD: Color = Color::Rgb(0xff, 0xc8, 0x3d);
const CYAN: Color = Color::Rgb(0x2e, 0xe6, 0xf6);
const MUTED: Color = Color::Rgb(0x7d, 0x8a, 0x99);

/// What a key press asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
}

/// View state for the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListView {
    pub selected: usize,
    pub show_help: bool,
}

impl ListView {
    /// Apply one key press. `lanes` is the number of rows shown.
    pub fn handle_key(&mut self, key: KeyEvent, lanes: usize) -> Action {
        let ctrl_c =
            key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl_c {
            return Action::Quit;
        }
        match key.code {
            // Esc closes the help first, then quits.
            KeyCode::Esc if self.show_help => self.show_help = false,
            KeyCode::Esc | KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(lanes.saturating_sub(1));
            }
            _ => {}
        }
        if lanes == 0 {
            self.selected = 0;
        }
        Action::None
    }
}

/// Draw the whole screen into `area`.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    world: &World,
    view: &ListView,
    source_error: Option<&str>,
) {
    let [title, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let waiting = world.waiting_count();
    let waiting_style = if waiting > 0 {
        Style::new().fg(GOLD).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(MUTED)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "runeway",
                Style::new().fg(CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" · {} · ", count_noun(world.lanes.len(), "agent"))),
            Span::styled(format!("{waiting} waiting"), waiting_style),
        ])),
        title,
    );

    let mut body = body;
    if let Some(error) = source_error {
        let [notice, rest] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(body);
        frame.render_widget(
            Paragraph::new(format!("agent-deck unavailable: {error}")).style(Style::new().fg(GOLD)),
            notice,
        );
        body = rest;
    }

    let rows: Vec<Row> = status_rows(world)
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let mark = if row.waiting {
                Span::styled("!", Style::new().fg(GOLD).add_modifier(Modifier::BOLD))
            } else {
                Span::raw(" ")
            };
            let context = row
                .context_percent
                .map(|percent| format!("{percent}%"))
                .unwrap_or_else(|| "-".to_string());
            let style = if index == view.selected {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                Style::new()
            };
            Row::new(vec![
                Line::from(mark),
                Line::from(row.title),
                Line::from(row.tool),
                Line::from(row.status),
                Line::from(row.action),
                Line::from(abbreviate_tokens(row.tokens)),
                Line::from(context),
            ])
            .style(style)
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Max(20),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Min(16),
            Constraint::Length(7),
            Constraint::Length(4),
        ],
    )
    .header(
        Row::new(["", "agent", "tool", "status", "doing", "tokens", "ctx"])
            .style(Style::new().fg(MUTED)),
    );
    frame.render_widget(table, body);

    frame.render_widget(
        Paragraph::new("↑↓ select · ? help · q quit").style(Style::new().fg(MUTED)),
        footer,
    );

    if view.show_help {
        render_help(frame, area);
    }
}

fn render_help(frame: &mut Frame, area: Rect) {
    let lines = [
        "↑ ↓  or  k j   select an agent",
        "?              show or hide this help",
        "q  Esc  Ctrl-C quit",
    ];
    let width = 40.min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(Block::new().borders(Borders::ALL).title(" keys ")),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};
    use runeway::model::{ActivityKind, Event, Session, SessionStatus, Tool, World};
    use std::path::PathBuf;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn world() -> World {
        let mut world = World::default();
        let session = |id: &str, tool, status| Session {
            id: id.into(),
            title: id.into(),
            path: PathBuf::from("/w"),
            tool,
            status,
            tmux_session: None,
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
        world.apply(Event::Session(session(
            "api",
            Tool::Codex,
            SessionStatus::Waiting,
        )));
        world
    }

    fn screen(world: &World, view: &ListView, error: Option<&str>) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), world, view, error))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn q_esc_and_ctrl_c_quit() {
        let mut view = ListView::default();
        assert_eq!(view.handle_key(key(KeyCode::Char('q')), 2), Action::Quit);
        assert_eq!(view.handle_key(key(KeyCode::Esc), 2), Action::Quit);
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL), 2),
            Action::Quit
        );
        assert_eq!(view.handle_key(key(KeyCode::Char('c')), 2), Action::None);
    }

    #[test]
    fn arrows_and_jk_move_selection_within_bounds() {
        let mut view = ListView::default();
        view.handle_key(key(KeyCode::Up), 3);
        assert_eq!(view.selected, 0);
        view.handle_key(key(KeyCode::Down), 3);
        view.handle_key(key(KeyCode::Char('j')), 3);
        view.handle_key(key(KeyCode::Char('j')), 3);
        assert_eq!(view.selected, 2);
        view.handle_key(key(KeyCode::Char('k')), 3);
        assert_eq!(view.selected, 1);
        view.handle_key(key(KeyCode::Down), 0);
        assert_eq!(view.selected, 0);
    }

    #[test]
    fn question_mark_toggles_help_and_esc_closes_it_first() {
        let mut view = ListView::default();
        view.handle_key(key(KeyCode::Char('?')), 2);
        assert!(view.show_help);
        assert_eq!(view.handle_key(key(KeyCode::Esc), 2), Action::None);
        assert!(!view.show_help);
    }

    #[test]
    fn renders_title_waiting_marker_and_footer() {
        let lines = screen(&world(), &ListView::default(), None);
        assert!(
            lines[0].contains("runeway · 2 agents · 1 waiting"),
            "{lines:#?}"
        );
        let api = lines.iter().find(|l| l.contains("api")).expect("api row");
        assert!(api.contains('!'), "{api}");
        assert!(api.contains("waiting for you"), "{api}");
        assert!(
            lines.iter().any(|l| l.contains("editing lib.rs")),
            "{lines:#?}"
        );
        assert!(lines[9].contains("q quit"), "{lines:#?}");
    }

    #[test]
    fn renders_source_error_and_help_overlay() {
        let view = ListView {
            selected: 0,
            show_help: true,
        };
        let lines = screen(&World::default(), &view, Some("not found"));
        let text = lines.join("\n");
        assert!(text.contains("agent-deck unavailable: not found"), "{text}");
        assert!(text.contains("Ctrl-C"), "{text}");
    }
}
