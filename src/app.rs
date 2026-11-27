//! The live view: one loop for input, snapshots from the source thread, and drawing.

use std::{
    io::{self, Stdout},
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    cursor::Show,
    event::{self, Event as TermEvent, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use runeway::{collector::Collector, demo, jump, model::World, sources::agentdeck::SessionLister};

use crate::{
    jumper,
    view::{Action, ListView},
    world_view::WorldView,
};

/// How often the source thread polls agent-deck and the transcripts.
const POLL: Duration = Duration::from_secs(1);
/// Frame interval while only idle loops run (about 8 FPS).
const IDLE_FRAME: Duration = Duration::from_millis(125);

/// What the source thread sends: the latest world and any source error.
type Snapshot = (World, Option<String>);

/// How the live view looks and paces itself.
#[derive(Debug, Clone)]
pub struct Options {
    pub lowres: bool,
    /// Frames per second while animating (already clamped).
    pub fps: u16,
    /// Where process information lives, for jumping to an agent.
    pub proc_root: PathBuf,
}

/// Where the world comes from: the collector thread, or the demo script.
enum Source {
    Live(Receiver<Snapshot>),
    Demo(Instant),
}

impl Source {
    /// Fold in anything new; true when the world or error changed.
    fn poll(&mut self, world: &mut World, error: &mut Option<String>) -> bool {
        match self {
            Source::Live(snapshots) => {
                let mut changed = false;
                // Keep only the newest snapshot.
                loop {
                    match snapshots.try_recv() {
                        Ok((next, next_error)) => {
                            changed |= next != *world || next_error != *error;
                            *world = next;
                            *error = next_error;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            let stopped = Some("the source thread stopped".to_string());
                            changed |= *error != stopped;
                            *error = stopped;
                            break;
                        }
                    }
                }
                changed
            }
            Source::Demo(start) => {
                let next = demo::world_at(ticks_since(*start));
                let changed = next != *world;
                *world = next;
                changed
            }
        }
    }
}

/// Animation ticks since `start`.
fn ticks_since(start: Instant) -> u64 {
    let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    ms.saturating_mul(demo::TICKS_PER_SECOND) / 1000
}

/// Whether to draw now: on new data or input at once; while animating at
/// `fps`; otherwise at the slow idle rate.
pub fn should_draw(
    changed: bool,
    animating: bool,
    input: bool,
    since_last: Duration,
    fps: u16,
) -> bool {
    if changed || input {
        return true;
    }
    since_last >= frame_interval(animating, fps)
}

fn frame_interval(animating: bool, fps: u16) -> Duration {
    if animating {
        Duration::from_secs(1) / u32::from(fps.max(1))
    } else {
        IDLE_FRAME
    }
}

/// Run the live view on real agents until the user quits.
pub fn run(
    collector: Collector,
    lister: Box<dyn SessionLister + Send>,
    options: Options,
) -> Result<()> {
    with_terminal(|terminal| {
        let source = Source::Live(spawn_sources(collector, lister));
        event_loop(terminal, source, options)
    })
}

/// Run the live view on the demo script until the user quits.
pub fn run_demo(options: Options) -> Result<()> {
    with_terminal(|terminal| event_loop(terminal, Source::Demo(Instant::now()), options))
}

fn with_terminal(
    body: impl FnOnce(&mut Terminal<CrosstermBackend<Stdout>>) -> Result<()>,
) -> Result<()> {
    install_panic_hook(restore_terminal);
    let _guard = TerminalGuard::enter()?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).context("starting the terminal")?;
    body(&mut terminal)
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut source: Source,
    options: Options,
) -> Result<()> {
    let mut view = ListView::default();
    let mut world_view = WorldView::new(options.lowres);
    let mut world = World::default();
    let mut source_error: Option<String> = None;
    let clock = Instant::now();
    let mut ticks_done: u64 = 0;
    let mut last_draw: Option<Instant> = None;
    let mut input = true;
    loop {
        let changed = source.poll(&mut world, &mut source_error);
        if changed {
            world_view.sync(&world);
            view.selected = view.selected.min(world.lanes.len().saturating_sub(1));
        }
        // The animation clock runs at 30 ticks a second whatever the frame rate.
        let due = ticks_since(clock).saturating_sub(ticks_done);
        for _ in 0..due.min(demo::TICKS_PER_SECOND) {
            world_view.tick();
        }
        ticks_done += due;

        let animating = world_view.animating();
        let since = last_draw.map_or(Duration::MAX, |at| at.elapsed());
        if should_draw(changed, animating, input, since, options.fps) {
            terminal
                .draw(|frame| {
                    world_view.render(frame, frame.area(), &world, &view, source_error.as_deref());
                })
                .context("drawing")?;
            last_draw = Some(Instant::now());
            input = false;
        }

        // Sleep until the next frame is due, waking early for a key.
        let since = last_draw.map_or(Duration::ZERO, |at| at.elapsed());
        let wait = frame_interval(animating, options.fps)
            .saturating_sub(since)
            .max(Duration::from_millis(1));
        if event::poll(wait).context("reading input")? {
            match event::read().context("reading input")? {
                TermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                    input = true;
                    match view.handle_key(key, world.lanes.len()) {
                        Action::Quit => return Ok(()),
                        Action::ToggleView => world_view.toggle(),
                        Action::Jump => {
                            if let Some(lane) = world.lanes.get(view.selected) {
                                let in_tmux = std::env::var_os("TMUX").is_some();
                                let plan = jump::plan(
                                    &lane.session,
                                    in_tmux,
                                    &options.proc_root,
                                    &jumper::window_pids(),
                                );
                                world_view.set_notice(jumper::run(&plan));
                            }
                        }
                        Action::TogglePause => world_view.toggle_pause(),
                        Action::None => {}
                    }
                }
                TermEvent::Resize(..) => input = true,
                _ => {}
            }
        }
    }
}

/// Poll the sources on a background thread. The thread only sends
/// snapshots; it never draws, and it stops when the receiver is dropped.
fn spawn_sources(
    mut collector: Collector,
    lister: Box<dyn SessionLister + Send>,
) -> Receiver<Snapshot> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        loop {
            collector.step(lister.as_ref());
            let snapshot = (
                collector.world().clone(),
                collector.source_error().map(str::to_string),
            );
            if sender.send(snapshot).is_err() {
                return;
            }
            thread::sleep(POLL);
        }
    });
    receiver
}

/// Raw mode and the alternate screen for as long as it lives.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<TerminalGuard> {
        enable_raw_mode().context("entering raw mode")?;
        if let Err(err) = execute!(io::stdout(), EnterAlternateScreen) {
            restore_terminal();
            return Err(err).context("entering the alternate screen");
        }
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// Put the user's terminal back: raw mode off, normal screen, cursor shown.
/// Safe to call more than once.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
}

/// Restore the terminal before any panic is reported, so the message is
/// readable and the shell is not left in raw mode.
fn install_panic_hook(restore: fn()) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    static RESTORED: AtomicBool = AtomicBool::new(false);

    fn fake_restore() {
        RESTORED.store(true, Ordering::SeqCst);
    }

    #[test]
    fn pacing_draws_only_when_needed() {
        let ms = Duration::from_millis;
        // New data or a key: draw at once.
        assert!(should_draw(true, false, false, ms(0), 30));
        assert!(should_draw(false, false, true, ms(0), 30));
        // Animating: one frame per 1/fps.
        assert!(!should_draw(false, true, false, ms(20), 30));
        assert!(should_draw(false, true, false, ms(34), 30));
        assert!(!should_draw(false, true, false, ms(60), 10));
        assert!(should_draw(false, true, false, ms(100), 10));
        // Idle: about 8 FPS.
        assert!(!should_draw(false, false, false, ms(40), 30));
        assert!(should_draw(false, false, false, IDLE_FRAME, 30));
    }

    #[test]
    fn panic_hook_restores_terminal_before_reporting() {
        let previous = std::panic::take_hook();
        // Silence the default report for this deliberate panic.
        std::panic::set_hook(Box::new(|_| {}));
        install_panic_hook(fake_restore);
        let result = std::panic::catch_unwind(|| panic!("boom"));
        std::panic::set_hook(previous);
        assert!(result.is_err());
        assert!(RESTORED.load(Ordering::SeqCst));
    }
}
