//! Draws the `--demo` world and list views as SVG screenshots for the
//! README: `cargo run --example screenshot [OUT_DIR]` (default
//! `docs/screenshots`). Reproducible: the demo is scripted and the lanes
//! are deterministic.

#[allow(dead_code)]
#[path = "../src/view.rs"]
mod view;
#[allow(dead_code)]
#[path = "../src/world_view.rs"]
mod world_view;

use std::{fmt::Write as _, path::PathBuf};

use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    style::{Color, Modifier},
};
use runeway::{demo, model::World};

/// 15 s into the demo: editing, a command running, a drone at the gate.
const TICK: u64 = 450;
const COLS: u16 = 120;
const ROWS: u16 = 52;
const LIST_ROWS: u16 = 8;
const CELL_W: u32 = 9;
const CELL_H: u32 = 18;
const BG: &str = "#0a0818";
const FG: &str = "#ecf0ff";

fn main() {
    let out = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("docs/screenshots"), PathBuf::from);
    std::fs::create_dir_all(&out).expect("output folder");

    let mut wv = world_view::WorldView::new(false);
    let mut world = World::default();
    for tick in 0..=TICK {
        let next = demo::world_at(tick);
        if next != world {
            world = next;
            wv.sync(&world);
        }
        wv.tick();
    }
    let list = view::ListView::default();
    // The list needs only a few rows.
    for (name, toggle, rows) in [("world.svg", false, ROWS), ("list.svg", true, LIST_ROWS)] {
        let mut terminal = Terminal::new(TestBackend::new(COLS, rows)).expect("terminal");
        if toggle {
            wv.toggle();
        }
        terminal
            .draw(|frame| wv.render(frame, frame.area(), &world, &list, None))
            .expect("draw");
        let svg = to_svg(terminal.backend().buffer());
        let path = out.join(name);
        std::fs::write(&path, svg).expect("write svg");
        println!("wrote {}", path.display());
    }
}

fn hex(color: Color, default: &str) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => default.to_string(),
    }
}

/// One cell's top colour, bottom colour, and the text to draw on it.
fn cell(buffer: &Buffer, x: u16, y: u16) -> (String, String, Option<(String, String, bool)>) {
    let c = &buffer[(x, y)];
    let (mut fg, mut bg) = (hex(c.fg, FG), hex(c.bg, BG));
    if c.modifier.contains(Modifier::REVERSED) {
        std::mem::swap(&mut fg, &mut bg);
    }
    match c.symbol() {
        "▀" => (fg, bg, None),
        " " | "" => (bg.clone(), bg, None),
        text => {
            let bold = c.modifier.contains(Modifier::BOLD);
            (bg.clone(), bg, Some((text.to_string(), fg, bold)))
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn to_svg(buffer: &Buffer) -> String {
    let (cols, rows) = (buffer.area.width, buffer.area.height);
    let (w, h) = (u32::from(cols) * CELL_W, u32::from(rows) * CELL_H);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n\
         <rect width=\"{w}\" height=\"{h}\" fill=\"{BG}\"/>\n"
    );
    let mut text = String::new();
    for y in 0..rows {
        let cells: Vec<_> = (0..cols).map(|x| cell(buffer, x, y)).collect();
        // Runs of one colour become one rect, per half-row.
        for (half, offset) in [(0, 0), (1, CELL_H / 2)] {
            let mut x = 0usize;
            while x < cells.len() {
                let color = if half == 0 { &cells[x].0 } else { &cells[x].1 };
                let mut end = x + 1;
                while end < cells.len()
                    && (if half == 0 {
                        &cells[end].0
                    } else {
                        &cells[end].1
                    }) == color
                {
                    end += 1;
                }
                if color != BG {
                    let _ = writeln!(
                        svg,
                        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{color}\"/>",
                        x as u32 * CELL_W,
                        u32::from(y) * CELL_H + offset,
                        (end - x) as u32 * CELL_W,
                        CELL_H / 2
                    );
                }
                x = end;
            }
        }
        for (x, (_, _, glyph)) in cells.iter().enumerate() {
            if let Some((symbol, fg, bold)) = glyph {
                let weight = if *bold { " font-weight=\"bold\"" } else { "" };
                let _ = writeln!(
                    text,
                    "<text x=\"{}\" y=\"{}\" fill=\"{fg}\"{weight}>{}</text>",
                    x as u32 * CELL_W,
                    u32::from(y) * CELL_H + 14,
                    escape(symbol)
                );
            }
        }
    }
    let _ = write!(
        svg,
        "<g font-family=\"ui-monospace, 'JetBrains Mono', monospace\" font-size=\"15\">\n{text}</g>\n</svg>\n"
    );
    svg
}
