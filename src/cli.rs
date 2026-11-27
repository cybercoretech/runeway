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

    /// Frames per second while animating. Used from phase 2.
    #[arg(long, global = true)]
    pub fps: Option<u16>,

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
