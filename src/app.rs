//! The live view: one loop for input, snapshots from the source thread, and drawing.

use std::{
    io::{self, Stdout},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    cursor::Show,
    event::{self, Event as TermEvent, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use runeway::{collector::Collector, model::World, sources::agentdeck::SessionLister};

use crate::view::{Action, ListView, render};

/// How often the source thread polls agent-deck and the transcripts.
const POLL: Duration = Duration::from_secs(1);
/// How long the loop waits for a key before checking for new snapshots.
const INPUT_WAIT: Duration = Duration::from_millis(100);

/// What the source thread sends: the latest world and any source error.
type Snapshot = (World, Option<String>);

/// Run the live view until the user quits.
pub fn run(collector: Collector, lister: Box<dyn SessionLister + Send>) -> Result<()> {
    install_panic_hook(restore_terminal);
    let _guard = TerminalGuard::enter()?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).context("starting the terminal")?;
    let snapshots = spawn_sources(collector, lister);
    event_loop(&mut terminal, &snapshots)
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    snapshots: &Receiver<Snapshot>,
) -> Result<()> {
    let mut view = ListView::default();
    let mut world = World::default();
    let mut source_error: Option<String> = None;
    loop {
        // Keep only the newest snapshot.
        loop {
            match snapshots.try_recv() {
                Ok((next, error)) => {
                    world = next;
                    source_error = error;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    source_error = Some("the source thread stopped".to_string());
                    break;
                }
            }
        }
        view.selected = view.selected.min(world.lanes.len().saturating_sub(1));
        terminal
            .draw(|frame| render(frame, frame.area(), &world, &view, source_error.as_deref()))
            .context("drawing")?;
        if event::poll(INPUT_WAIT).context("reading input")?
            && let TermEvent::Key(key) = event::read().context("reading input")?
            && key.kind == KeyEventKind::Press
            && view.handle_key(key, world.lanes.len()) == Action::Quit
        {
            return Ok(());
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
