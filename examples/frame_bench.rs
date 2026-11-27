//! Frame-time benchmark: a 12-lane demo world drawn at 160×48 cells, 300
//! frames. Run it in release mode:
//!
//! ```text
//! cargo run --release --example frame_bench
//! ```
//!
//! The target is under 4 ms per frame.

#[allow(dead_code)]
#[path = "../src/view.rs"]
mod view;
#[allow(dead_code)]
#[path = "../src/world_view.rs"]
mod world_view;

use std::time::Instant;

use ratatui::{Terminal, backend::TestBackend};
use runeway::{demo, model::World};

const FRAMES: usize = 300;

fn main() {
    // Twelve lanes: the four demo agents, three times over.
    let mut world = World::default();
    for copy in 0..3 {
        for mut lane in demo::world_at(400).lanes {
            lane.session.id = format!("{}-{copy}", lane.session.id);
            lane.session.title = format!("{}-{copy}", lane.session.title);
            world.lanes.push(lane);
        }
    }
    let mut terminal = Terminal::new(TestBackend::new(160, 48)).expect("terminal");
    let mut wv = world_view::WorldView::new(false);
    let list = view::ListView::default();
    wv.sync(&world);

    let mut times = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        let start = Instant::now();
        wv.tick();
        terminal
            .draw(|frame| wv.render(frame, frame.area(), &world, &list, None))
            .expect("draw");
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    let p95 = times[times.len() * 95 / 100];
    println!(
        "{} lanes, 160×48 cells, {FRAMES} frames: mean {mean:.3} ms, p95 {p95:.3} ms",
        world.lanes.len()
    );
}
