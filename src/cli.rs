//! Command-line flags. Knows flags only; `main` does the wiring.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// A retro side-scrolling view of your running coding agents.
///
/// Reads agent-deck's session list and the agents' transcripts. Never writes
/// to them and never uses the network.
#[derive(Debug, Parser)]
#[command(name = "runeway", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Use one pixel per cell (for SSH and slow terminals).
    #[arg(long, global = true)]
    pub lowres: bool,

    /// Frames per second while animating, 5 to 60 [default: 30].
    #[arg(long, global = true)]
    pub fps: Option<u16>,

    /// Play four scripted agents instead of watching real ones.
    #[arg(long, global = true)]
    pub demo: bool,

    /// Do not ask agent-deck for sessions.
    #[arg(long, global = true)]
    pub no_agent_deck: bool,

    /// Do not show Claude and Codex agents running outside agent-deck.
    #[arg(long, global = true)]
    pub no_discover: bool,

    /// The agent-deck program [default: agent-deck found on PATH].
    #[arg(long, global = true, value_name = "PATH")]
    pub agent_deck: Option<PathBuf>,

    /// Claude Code's projects folder [default: ~/.claude/projects].
    #[arg(long, global = true, value_name = "DIR")]
    pub claude_dir: Option<PathBuf>,

    /// Codex's sessions folder [default: ~/.codex/sessions].
    #[arg(long, global = true, value_name = "DIR")]
    pub codex_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// The live view (default)
    Watch,
    /// Print one line per agent and exit
    Status {
        /// Print JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Report which sources were found; exits 1 if agent-deck cannot be asked
    Check,
}

/// The animating frame rate: `--fps` clamped to 5–60, 30 by default.
pub fn frame_rate(fps: Option<u16>) -> u16 {
    fps.unwrap_or(30).clamp(5, 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_flag_is_clamped() {
        assert_eq!(frame_rate(None), 30);
        assert_eq!(frame_rate(Some(1)), 5);
        assert_eq!(frame_rate(Some(24)), 24);
        assert_eq!(frame_rate(Some(240)), 60);
    }

    #[test]
    fn demo_and_fps_flags_parse() {
        let cli = Cli::try_parse_from(["runeway", "--demo", "--fps", "12"]).expect("parse");
        assert!(cli.demo);
        assert_eq!(cli.fps, Some(12));
    }
}
