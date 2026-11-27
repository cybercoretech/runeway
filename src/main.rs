//! `runeway`: wiring only. Parse flags, build the config and sources, run.

mod app;
mod cli;
mod view;
mod world_view;

use std::{
    env,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, Result};
use clap::Parser;
use runeway::{
    collector::{Collector, check},
    config::Config,
    sources::{
        agentdeck::{AgentDeck, SessionLister, Unavailable},
        discover::{Combined, Discovery},
    },
    status,
};

use cli::{Cli, Command};

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("runeway: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;
    let mut config = Config::from_home(&home);
    if let Some(dir) = cli.claude_dir.clone() {
        config.claude_projects = dir;
    }
    if let Some(dir) = cli.codex_dir.clone() {
        config.codex_sessions = dir;
    }
    if !cli.no_agent_deck {
        config.agent_deck = cli
            .agent_deck
            .clone()
            .or_else(|| find_on_path("agent-deck"));
    }
    let deck: Box<dyn SessionLister + Send> = match (&config.agent_deck, cli.no_agent_deck) {
        (_, true) => Box::new(Unavailable("turned off with --no-agent-deck".to_string())),
        (Some(program), false) => Box::new(AgentDeck::new(program.clone())),
        (None, false) => Box::new(Unavailable(
            "not found on PATH (give --agent-deck PATH)".to_string(),
        )),
    };
    let command = cli.command.unwrap_or(Command::Watch);
    // `check` reports agent-deck itself, so it asks agent-deck alone.
    if let Command::Check = command {
        return Ok(run_check(&config, deck.as_ref()));
    }
    let lister: Box<dyn SessionLister + Send> = if cli.no_discover {
        deck
    } else {
        let discovery = Discovery::new(
            config.claude_projects.clone(),
            config.codex_sessions.clone(),
            config.discover_within,
        );
        Box::new(Combined::new(deck, discovery))
    };

    match command {
        Command::Watch => {
            let options = app::Options {
                lowres: cli.lowres,
                fps: cli::frame_rate(cli.fps),
            };
            if cli.demo {
                app::run_demo(options)?;
            } else {
                app::run(Collector::new(config), lister, options)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Status { json: false } => {
            let mut collector = Collector::new(config);
            collector.step(lister.as_ref());
            print!(
                "{}",
                status::render_text(collector.world(), collector.source_error())
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Status { json: true } => {
            let mut collector = Collector::new(config);
            collector.step(lister.as_ref());
            println!(
                "{}",
                status::render_json(collector.world(), collector.source_error())
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Check => unreachable!("handled above"),
    }
}

/// `runeway check`: one line per source; exit 1 if agent-deck cannot be asked.
fn run_check(config: &Config, lister: &dyn SessionLister) -> ExitCode {
    let report = check(config, Some(lister));
    let dir_line = |name: &str, found: bool, dir: &Path| {
        if found {
            println!("{name:<11} ok  {}", dir.display());
        } else {
            println!("{name:<11} missing: {} not found", dir.display());
        }
    };
    match &report.agent_deck {
        Ok(count) => println!("agent-deck  ok ({})", status::count_noun(*count, "session")),
        Err(reason) => println!("agent-deck  missing: {reason}"),
    }
    dir_line("claude", report.claude_dir, &config.claude_projects);
    dir_line("codex", report.codex_dir, &config.codex_sessions);
    if report.agent_deck.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// The first executable `name` on `PATH`.
fn find_on_path(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}
