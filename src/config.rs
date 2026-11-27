//! Where Runeway looks for its sources. No paths are hard-coded in the
//! library: the binary builds a `Config` from `$HOME` and command-line flags.

use std::path::{Path, PathBuf};

/// Resolved source locations and options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Claude Code's `projects` folder (one sub-folder per working directory).
    pub claude_projects: PathBuf,
    /// Codex's `sessions` folder (`YYYY/MM/DD/rollout-*.jsonl`).
    pub codex_sessions: PathBuf,
    /// The `agent-deck` program, if one was found or given.
    pub agent_deck: Option<PathBuf>,
    /// Context window used for Claude's context meter, in tokens.
    pub claude_context_window: u64,
    /// How much of an existing transcript to read on start, in bytes.
    pub backfill_bytes: u64,
}

impl Config {
    /// Default locations under `home`. `agent_deck` is left for the caller
    /// to fill in (it is looked up on `PATH`).
    pub fn from_home(home: &Path) -> Config {
        Config {
            claude_projects: home.join(".claude/projects"),
            codex_sessions: home.join(".codex/sessions"),
            agent_deck: None,
            claude_context_window: 200_000,
            backfill_bytes: 256 * 1024,
        }
    }
}
