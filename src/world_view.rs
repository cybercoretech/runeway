//! The platformer view: one lane per agent, drawn with half-block pixels,
//! with a HUD row per lane. The phase 1 list stays one key (`l`) away.

use std::collections::HashMap;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use runeway::{
    model::{LaneState, World},
    status::{abbreviate_tokens, action_label},
    world::{
        Rgb,
        halfblock::{pack, pack_lowres},
        lane::LaneSim,
        palette::{SKY, color},
        pixels::Pixels,
    },
};

use crate::view::{self, ListView};

/// Terminal rows per lane: one HUD row and the world below it.
pub const LANE_ROWS: u16 = 12;
/// World rows per lane (22 half-block pixels).
pub const WORLD_ROWS: u16 = LANE_ROWS - 1;
/// The border of a lane that waits for the owner.
pub const WAITING: Color = Color::Rgb(0xff, 0xcc, 0x40);
const SELECTED: Color = Color::Rgb(0x40, 0xe0, 0xf0);
const MUTED: Color = Color::Rgb(0x7d, 0x8a, 0x99);
/// Narrowest terminal the world is drawn in.
const MIN_WIDTH: u16 = 24;
/// Segments in the context meter.
const METER: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    World,
    List,
}

pub struct WorldView {
    pub sims: HashMap<String, LaneSim>,
    pub mode: ViewMode,
    pub lowres: bool,
}

/// Which lanes are drawn in full and which as one compact line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Layout {
    full: Vec<usize>,
    compact: Vec<usize>,
    /// Lanes not on screen at all.
    more: usize,
}

/// Fit `n` lanes into `rows` rows, keeping `selected` in full: as many full
/// lanes as leave room for one compact line per remaining lane, and at
/// least one full lane when there is room for it.
fn layout(n: usize, rows: u16, selected: usize) -> Layout {
    let rows = usize::from(rows);
    let lane = usize::from(LANE_ROWS);
    let mut k = n.min(rows / lane);
    while k > 1 && k * lane + (n - k) > rows {
        k -= 1;
    }
    let selected = selected.min(n.saturating_sub(1));
    let start = (selected + 1).saturating_sub(k).min(n - k);
    let full: Vec<usize> = (start..start + k).collect();
    let space = rows.saturating_sub(k * lane);
    let rest: Vec<usize> = (0..n).filter(|i| !full.contains(i)).collect();
    let compact: Vec<usize> = rest.into_iter().take(space).collect();
    let more = n - k - compact.len();
    Layout {
        full,
        compact,
        more,
    }
}

impl WorldView {
    pub fn new(lowres: bool) -> WorldView {
        WorldView {
            sims: HashMap::new(),
            mode: ViewMode::World,
            lowres,
        }
    }

    pub fn toggle(&mut self) {
        self.mode = match self.mode {
            ViewMode::World => ViewMode::List,
            ViewMode::List => ViewMode::World,
        };
    }

    /// Follow the world: new lanes get a simulation, gone ones lose theirs.
    pub fn sync(&mut self, world: &World) {
        self.sims
            .retain(|id, _| world.lanes.iter().any(|lane| &lane.session.id == id));
        for lane in &world.lanes {
            match self.sims.get_mut(&lane.session.id) {
                Some(sim) => sim.sync(lane),
                None => {
                    self.sims
                        .insert(lane.session.id.clone(), LaneSim::new(lane));
                }
            }
        }
    }

    /// True while any lane on the world view moves (drawn at full rate).
    pub fn animating(&self) -> bool {
        self.mode == ViewMode::World && self.sims.values().any(LaneSim::is_animating)
    }

    /// Advance every lane one animation tick.
    pub fn tick(&mut self) {
        for sim in self.sims.values_mut() {
            sim.tick();
        }
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        world: &World,
        view: &ListView,
        source_error: Option<&str>,
    ) {
        if self.mode == ViewMode::List {
            view::render(frame, area, world, view, source_error);
            return;
        }
        let notice_rows = u16::from(source_error.is_some());
        let short = area.height < LANE_ROWS + 2 + notice_rows;
        let narrow = area.width < MIN_WIDTH;
        if short || narrow {
            let what = match (short, narrow) {
                (true, true) => "taller and wider",
                (true, false) => "taller",
                _ => "wider",
            };
            frame.render_widget(
                Paragraph::new(format!("runeway: make the window {what}"))
                    .wrap(Wrap { trim: true })
                    .style(Style::new().fg(MUTED)),
                area,
            );
            return;
        }

        let mut y = area.y;
        frame.render_widget(Paragraph::new(view::top_bar(world)), row(area, y));
        y += 1;
        if let Some(error) = source_error {
            frame.render_widget(view::source_notice(error), row(area, y));
            y += 1;
        }
        let footer_y = area.bottom() - 1;
        let body_rows = footer_y - y;

        if world.lanes.is_empty() {
            frame.render_widget(
                Paragraph::new("no agents running yet").style(Style::new().fg(MUTED)),
                row(area, y),
            );
        }
        let plan = layout(world.lanes.len(), body_rows, view.selected);
        for &index in &plan.full {
            let lane = &world.lanes[index];
            self.draw_lane(frame.buffer_mut(), area, y, lane, index == view.selected);
            y += LANE_ROWS;
        }
        for &index in &plan.compact {
            let lane = &world.lanes[index];
            frame.render_widget(Paragraph::new(compact_line(lane)), row(area, y));
            y += 1;
        }

        let more = if plan.more > 0 {
            format!("+{} more · ", plan.more)
        } else {
            String::new()
        };
        frame.render_widget(
            Paragraph::new(format!("↑↓ select · {more}l list · ? help · q quit"))
                .style(Style::new().fg(MUTED)),
            row(area, footer_y),
        );
        if view.show_help {
            view::render_help(frame, area);
        }
    }

    /// One lane at row `y`: gutters, the HUD row, then the world pixels.
    fn draw_lane(&self, buf: &mut Buffer, area: Rect, y: u16, lane: &LaneState, selected: bool) {
        let waiting = lane.session.status == runeway::model::SessionStatus::Waiting;
        let edge = if waiting {
            Some(WAITING)
        } else if selected {
            Some(SELECTED)
        } else {
            None
        };
        if let Some(edge) = edge {
            for dy in 0..LANE_ROWS {
                buf[(area.x, y + dy)].set_symbol("▌").set_fg(edge);
                buf[(area.right() - 1, y + dy)].set_symbol("▐").set_fg(edge);
            }
        }
        let inner = Rect {
            x: area.x + 1,
            y,
            width: area.width - 2,
            height: 1,
        };
        buf.set_line(inner.x, inner.y, &hud_line(lane, waiting), inner.width);

        let Some(sim) = self.sims.get(&lane.session.id) else {
            return;
        };
        let width = usize::from(inner.width);
        let mut px = Pixels::new(width, usize::from(WORLD_ROWS) * 2, color(SKY));
        sim.render(&mut px);
        let top = y + 1;
        let rgb = |c: Rgb| Color::Rgb(c[0], c[1], c[2]);
        if self.lowres {
            for (dy, cells) in pack_lowres(&px).iter().enumerate() {
                for (dx, c) in cells.iter().enumerate() {
                    buf[(inner.x + dx as u16, top + dy as u16)]
                        .set_symbol(" ")
                        .set_bg(rgb(*c));
                }
            }
        } else {
            for (dy, cells) in pack(&px).iter().enumerate() {
                for (dx, cell) in cells.iter().enumerate() {
                    buf[(inner.x + dx as u16, top + dy as u16)]
                        .set_symbol("▀")
                        .set_fg(rgb(cell.top))
                        .set_bg(rgb(cell.bottom));
                }
            }
        }
    }
}

fn row(area: Rect, y: u16) -> Rect {
    Rect {
        x: area.x,
        y,
        width: area.width,
        height: 1,
    }
}

/// `▰▰▰▱▱`: the context left, draining as it fills.
fn meter(context_used: Option<f32>) -> String {
    let left = context_used.map_or(0, |used| {
        ((1.0 - used.clamp(0.0, 1.0)) * METER as f32).round() as usize
    });
    "▰".repeat(left) + &"▱".repeat(METER - left)
}

fn hud_line(lane: &LaneState, waiting: bool) -> Line<'static> {
    let action_style = if waiting {
        Style::new().fg(WAITING).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    Line::from(vec![
        Span::styled(
            format!(" {} ", lane.session.title),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} ", lane.session.tool.name()),
            Style::new().fg(MUTED),
        ),
        Span::styled(
            format!(" ◈ {} ", abbreviate_tokens(lane.tokens_total)),
            Style::new().fg(WAITING),
        ),
        Span::styled(
            format!(" {} ", meter(lane.context_used)),
            Style::new().fg(SELECTED),
        ),
        Span::styled(format!(" {}", action_label(lane)), action_style),
    ])
}

fn compact_line(lane: &LaneState) -> Line<'static> {
    let waiting = lane.session.status == runeway::model::SessionStatus::Waiting;
    let mark = if waiting {
        Span::styled(" ! ", Style::new().fg(WAITING).add_modifier(Modifier::BOLD))
    } else {
        Span::raw("   ")
    };
    Line::from(vec![
        mark,
        Span::styled(
            format!("{} ", lane.session.title),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} ", lane.session.tool.name()),
            Style::new().fg(MUTED),
        ),
        Span::raw(format!("◈ {} ", abbreviate_tokens(lane.tokens_total))),
        Span::raw(action_label(lane)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Action;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
    use runeway::model::{ActivityKind, Event, Session, SessionStatus, Tool};
    use std::path::PathBuf;

    fn session(id: &str, tool: Tool, status: SessionStatus) -> Session {
        Session {
            id: id.into(),
            title: id.into(),
            path: PathBuf::from("/w"),
            tool,
            status,
            tmux_session: None,
            transcript: None,
        }
    }

    fn world() -> World {
        let mut world = World::default();
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
            context_used: Some(0.31),
        });
        world.apply(Event::Session(session(
            "api",
            Tool::Codex,
            SessionStatus::Waiting,
        )));
        world.apply(Event::Usage {
            id: "api".into(),
            tokens_total: 1_000,
            context_used: None,
        });
        world
    }

    fn many(n: usize) -> World {
        let mut world = World::default();
        for i in 0..n {
            world.apply(Event::Session(session(
                &format!("agent{i:02}"),
                Tool::Claude,
                SessionStatus::Running,
            )));
        }
        world
    }

    fn draw(
        terminal: &mut Terminal<TestBackend>,
        world: &World,
        wv: &mut WorldView,
        view: &ListView,
    ) -> Buffer {
        wv.sync(world);
        wv.tick();
        terminal
            .draw(|frame| wv.render(frame, frame.area(), world, view, None))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn screen(
        w: u16,
        h: u16,
        world: &World,
        wv: &mut WorldView,
        view: &ListView,
    ) -> (Vec<String>, Buffer) {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
        let buffer = draw(&mut terminal, world, wv, view);
        (lines(&buffer), buffer)
    }

    fn lines(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn top_bar_counts_and_tokens() {
        let (lines, _) = screen(
            100,
            30,
            &world(),
            &mut WorldView::new(false),
            &ListView::default(),
        );
        assert!(
            lines[0].contains("runeway · 2 agents · 1 waiting"),
            "{}",
            lines[0]
        );
        assert!(lines[0].contains("Σ 13.4k tokens"), "{}", lines[0]);
    }

    #[test]
    fn hud_row_shows_name_tokens_meter_and_action() {
        let (lines, _) = screen(
            100,
            30,
            &world(),
            &mut WorldView::new(false),
            &ListView::default(),
        );
        let hud = lines.iter().find(|l| l.contains("app")).expect("app hud");
        assert!(hud.contains("claude"), "{hud}");
        assert!(hud.contains("12.4k"), "{hud}");
        assert!(hud.contains("▰▰▰▱▱"), "{hud}");
        assert!(hud.contains("editing lib.rs"), "{hud}");
    }

    #[test]
    fn world_rows_are_half_block_pixels() {
        let (_, buffer) = screen(
            100,
            30,
            &world(),
            &mut WorldView::new(false),
            &ListView::default(),
        );
        // Row 2 is the first world row of the first lane (row 1 is its HUD).
        let cell = &buffer[(10, 2)];
        assert_eq!(cell.symbol(), "▀");
        assert!(matches!(cell.fg, Color::Rgb(..)) && matches!(cell.bg, Color::Rgb(..)));
    }

    #[test]
    fn waiting_lane_border_is_gold() {
        let (lines, buffer) = screen(
            100,
            30,
            &world(),
            &mut WorldView::new(false),
            &ListView::default(),
        );
        let hud_row = lines
            .iter()
            .position(|l| l.contains("api"))
            .expect("api hud") as u16;
        for y in hud_row..hud_row + LANE_ROWS {
            assert_eq!(buffer[(0, y)].fg, WAITING, "left border row {y}");
            assert_eq!(buffer[(99, y)].fg, WAITING, "right border row {y}");
        }
        let app_row = lines
            .iter()
            .position(|l| l.contains("app"))
            .expect("app hud") as u16;
        assert_ne!(buffer[(0, app_row)].fg, WAITING);
    }

    #[test]
    fn more_lanes_than_fit_fall_back_with_more_marker() {
        let world = many(6);
        let view = ListView {
            selected: 5,
            show_help: false,
        };
        let (lines, _) = screen(80, 40, &world, &mut WorldView::new(false), &view);
        let text = lines.join("\n");
        for i in 0..6 {
            assert!(
                text.contains(&format!("agent{i:02}")),
                "agent{i:02} missing:\n{text}"
            );
        }
        // Everyone is on screen (two in full, four as lines): nothing is hidden.
        assert!(!lines[39].contains("more"), "{text}");
        // The selected agent is one of the full lanes (its HUD is followed by pixels).
        let row = lines
            .iter()
            .position(|l| l.contains("agent05"))
            .expect("selected");
        assert!(
            lines[row + 1].contains('▀'),
            "selected lane not drawn in full:\n{text}"
        );
    }

    #[test]
    fn hundreds_of_agents_still_fit() {
        let world = many(200);
        let (lines, _) = screen(
            80,
            30,
            &world,
            &mut WorldView::new(false),
            &ListView::default(),
        );
        // 28 body rows: 1 full lane (12) + 16 lines; 183 not shown at all.
        assert!(lines[29].contains("+183 more"), "{}", lines[29]);
    }

    #[test]
    fn tiny_terminal_shows_notice_not_panic() {
        for (w, h) in [(20, 5), (40, 8)] {
            let (lines, _) = screen(
                w,
                h,
                &world(),
                &mut WorldView::new(false),
                &ListView::default(),
            );
            let text = lines.join(" ");
            assert!(text.contains("taller"), "{w}×{h}: {text}");
        }
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).expect("terminal");
        let mut wv = WorldView::new(false);
        for (w, h) in [
            (80, 30),
            (20, 5),
            (120, 50),
            (3, 1),
            (1, 1),
            (80, 13),
            (80, 14),
            (81, 15),
        ] {
            terminal.backend_mut().resize(w, h);
            draw(&mut terminal, &world(), &mut wv, &ListView::default());
        }
    }

    #[test]
    fn l_toggles_list_view() {
        let mut view = ListView::default();
        let action = view.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE), 2);
        assert_eq!(action, Action::ToggleView);
        let mut wv = WorldView::new(false);
        wv.toggle();
        assert_eq!(wv.mode, ViewMode::List);
        let (lines, _) = screen(100, 30, &world(), &mut wv, &view);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("agent") && l.contains("doing")),
            "{lines:#?}"
        );
        wv.toggle();
        assert_eq!(wv.mode, ViewMode::World);
    }

    #[test]
    fn lowres_uses_one_pixel_per_cell() {
        let (_, buffer) = screen(
            100,
            30,
            &world(),
            &mut WorldView::new(true),
            &ListView::default(),
        );
        let cell = &buffer[(10, 2)];
        assert_eq!(cell.symbol(), " ");
        assert!(matches!(cell.bg, Color::Rgb(..)));
    }

    #[test]
    fn empty_world_says_so() {
        let (lines, _) = screen(
            100,
            30,
            &World::default(),
            &mut WorldView::new(false),
            &ListView::default(),
        );
        assert!(lines.join("\n").contains("no agents"), "{lines:#?}");
    }
}
