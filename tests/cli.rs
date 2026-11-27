//! End-to-end tests of the `runeway` binary against fixture sources.

use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Run runeway with an isolated HOME and empty transcript folders.
fn runeway(args: &[&str], agent_deck: &Path) -> Output {
    runeway_seeded(args, agent_deck, |_| {})
}

/// `runeway`, after `seed` has filled the Claude projects folder.
fn runeway_seeded(args: &[&str], agent_deck: &Path, seed: impl FnOnce(&Path)) -> Output {
    let home = tempfile::tempdir().expect("home");
    let claude = home.path().join("claude");
    let codex = home.path().join("codex");
    std::fs::create_dir_all(&claude).expect("claude dir");
    std::fs::create_dir_all(&codex).expect("codex dir");
    seed(&claude);
    Command::new(env!("CARGO_BIN_EXE_runeway"))
        .args(args)
        .arg("--agent-deck")
        .arg(agent_deck)
        .arg("--claude-dir")
        .arg(&claude)
        .arg("--codex-dir")
        .arg(&codex)
        .env("HOME", home.path())
        .output()
        .expect("run runeway")
}

fn fake_agent_deck() -> PathBuf {
    fixtures().join("bin/fake-agent-deck")
}

fn missing_agent_deck() -> PathBuf {
    fixtures().join("bin/no-such-agent-deck")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn status_lists_fixture_agents_and_waiting_count() {
    let output = runeway(&["status"], &fake_agent_deck());
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("app"), "{text}");
    assert!(text.contains("api"), "{text}");
    assert!(text.contains("docs"), "{text}");
    assert!(!text.contains("old"), "archived session shown: {text}");
    assert_eq!(text.lines().last(), Some("3 agents, 1 waiting"));
}

#[test]
fn status_json_is_valid_and_has_waiting_1() {
    let output = runeway(&["status", "--json"], &fake_agent_deck());
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("json");
    assert_eq!(value["waiting"], 1);
    assert_eq!(value["agents"].as_array().map(Vec::len), Some(3));
    assert!(value["agent_deck_error"].is_null());
}

#[test]
fn check_reports_sources_and_exits_0() {
    let output = runeway(&["check"], &fake_agent_deck());
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(
        text.contains("agent-deck") && text.contains("ok (3 sessions)"),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|l| l.starts_with("claude") && l.contains("ok")),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|l| l.starts_with("codex") && l.contains("ok")),
        "{text}"
    );
}

#[test]
fn missing_agent_deck_status_exits_0_with_message() {
    let output = runeway(&["status"], &missing_agent_deck());
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.starts_with("agent-deck unavailable:"), "{text}");
    assert_eq!(text.lines().last(), Some("0 agents, 0 waiting"));
}

#[test]
fn missing_agent_deck_check_exits_1() {
    let output = runeway(&["check"], &missing_agent_deck());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stdout(&output).contains("agent-deck"), "{output:?}");
}

#[test]
fn help_lists_the_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_runeway"))
        .arg("--help")
        .output()
        .expect("run");
    let text = stdout(&output);
    for command in ["watch", "status", "check"] {
        assert!(text.contains(command), "{text}");
    }
}

fn seed_recent_claude_transcript(claude: &Path) {
    let dir = claude.join("-w-solo-agent");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("s1.jsonl"), "{}\n").expect("write");
}

#[test]
fn status_shows_agents_found_outside_agent_deck() {
    let output = runeway_seeded(
        &["status"],
        &fake_agent_deck(),
        seed_recent_claude_transcript,
    );
    let text = stdout(&output);
    assert!(text.contains("agent"), "{text}");
    assert_eq!(text.lines().last(), Some("4 agents, 1 waiting"));
}

#[test]
fn no_discover_leaves_them_out() {
    let output = runeway_seeded(
        &["status", "--no-discover"],
        &fake_agent_deck(),
        seed_recent_claude_transcript,
    );
    assert_eq!(stdout(&output).lines().last(), Some("3 agents, 1 waiting"));
}

/// Run `runeway status` with no agent-deck on PATH and none given.
fn status_without_agent_deck_on_path(extra: &[&str]) -> String {
    let home = tempfile::tempdir().expect("home");
    let empty_path = home.path().join("bin");
    std::fs::create_dir_all(&empty_path).expect("bin dir");
    let output = Command::new(env!("CARGO_BIN_EXE_runeway"))
        .arg("status")
        .args(extra)
        .arg("--claude-dir")
        .arg(home.path().join("claude"))
        .arg("--codex-dir")
        .arg(home.path().join("codex"))
        .env("HOME", home.path())
        .env("PATH", &empty_path)
        .output()
        .expect("run runeway");
    assert!(output.status.success(), "{output:?}");
    stdout(&output)
}

#[test]
fn turning_agent_deck_off_shows_no_notice() {
    let text = status_without_agent_deck_on_path(&["--no-agent-deck"]);
    assert!(!text.contains("agent-deck unavailable"), "{text}");
}

#[test]
fn agent_deck_not_installed_is_quiet_while_discovering() {
    let text = status_without_agent_deck_on_path(&[]);
    assert!(!text.contains("agent-deck unavailable"), "{text}");
}

#[test]
fn agent_deck_not_installed_without_discovery_still_explains() {
    let text = status_without_agent_deck_on_path(&["--no-discover"]);
    assert!(text.starts_with("agent-deck unavailable:"), "{text}");
}
