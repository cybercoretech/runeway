# Runeway Phase 1: Telemetry and Status List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A working `runeway` binary that discovers the owner's agent sessions through agent-deck, follows Claude and Codex transcripts live, and shows every agent's status, current action, tokens and context in `runeway status [--json]`, `runeway check`, and a plain ratatui list view.

**Architecture:** One crate with a library (event model, parsers, collector; no terminal code) and a thin binary (CLI wiring and the TUI). Sources are adapters that turn agent-deck JSON and transcript lines into one `Event` type; a `Collector` folds events into per-session `LaneState`; the CLI and TUI only render that state. The platformer renderer (Phase 2) and full mapping (Phase 3) get their own plans after this one merges.

**Tech Stack:** Rust 1.98.1 stable; ratatui 0.30, crossterm 0.29, clap 4.6 (derive), serde 1 + serde_json 1, thiserror 2, anyhow 1; dev: tempfile 3.

**Spec:** `docs/superpowers/specs/2026-11-27-runeway-design.md`

## Global Constraints

- Strictly read-only: never write to, copy or send transcripts or agent-deck data; no network access anywhere in the crate.
- `Activity.detail` is a short label only (tool name, file name, first word of a command). It never contains file contents, full command lines, prompts or outputs.
- Library code: `thiserror` errors, `Result` returns, no `unwrap`/`expect` on recoverable paths, no terminal code. `anyhow` only in the binary.
- No paths as constants in the library. Every location comes from `Config` (defaults resolved from `$HOME` in the binary; overridable by flags).
- `rust-version` in `Cargo.toml` = the highest `rust-version` among the chosen dependencies (read from `cargo metadata` in Task 1), not higher.
- TUI quits on `q`, `Esc` and `Ctrl-C`; the terminal is restored on normal exit and on panic.
- Original identity only: nothing named or styled after Nintendo or any other game's characters (applies to text and sample data too).
- Git: new local repo at `ai/tools/runeway`, no remote. Work on branch `feat/phase1-telemetry`; commit after each task; conventional commit types; body `Project contributor`; footer `👻 Code & Design by Darkstardevx 🤖 Generated with Claude Code` then `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Commit email must be `19244308+darkstardevx@users.noreply.github.com`.
- Verification commands (all must pass at the end of every task that touches code): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

## Facts from the owner's machine (do not re-derive)

- Claude transcripts: `~/.claude/projects/<slug>/<session-id>.jsonl`. `<slug>` = the session's working directory with every character outside `[A-Za-z0-9]` replaced by `-` (`/home/user/.cache/tmp` → `-home-user--cache-tmp`). Lines are JSON objects with a `type`. Assistant lines carry `message.content[]` items `{ "type": "tool_use", "id", "name", "input": {...} }` and `message.usage { input_tokens, cache_creation_input_tokens, cache_read_input_tokens, output_tokens }`. User lines carry `message.content[]` items `{ "type": "tool_result", "tool_use_id", "is_error"?, "content" }`. Many other line types exist and must be ignored.
- Claude tool names seen: `Read`, `Write`, `Edit`, `Bash` (input `command`), `Grep`, `Glob`, `ToolSearch`, `Agent`, and `mcp__<server>__<tool>`.
- Codex transcripts: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. First line `{ "type": "session_meta", "payload": { "cwd", "session_id", "timestamp", ... } }`. Commands: `{ "type": "response_item", "payload": { "type": "custom_tool_call", "name": "exec", "call_id", "input" } }`, results `{ "type": "response_item", "payload": { "type": "custom_tool_call_output", "call_id", "output" } }`. Tokens: `{ "type": "event_msg", "payload": { "type": "token_count", "info": { "total_token_usage": { "total_tokens", ... }, "last_token_usage": { "input_tokens", ... }, "model_context_window" } } }`.
- `agent-deck ls --json` prints an array of `{ id, title, path, group, tool, command, status, status_source, substate, tmux_session, profile, created_at, archived, last_activity_at }`. `status` values: `running`, `busy`, `waiting`, `idle`, `ready`, `stopped`, `error`. `waiting` is the only signal for "waiting for permission".

## Review Focus

1. A transcript line that is only half written when read (no trailing newline yet): it must be neither parsed twice nor lost; it is read once complete.
2. A very large existing transcript when Runeway starts: start from the end (read at most the last 256 KiB to rebuild current state), never replay the whole history.
3. agent-deck not installed, failing, or printing invalid JSON: `status` and the TUI still run, show "agent-deck unavailable: <reason>" and exit 0; `check` reports it and exits 1.
4. A session whose transcript cannot be matched (other agents, unknown paths): the row still appears with agent-deck status and `transcript: none`.
5. A panic inside the TUI loop: the terminal is restored (raw mode off, alternate screen left, cursor shown).

Each has a test in the owning task (Tasks 2, 2, 4, 5, 7).

---

### Task 1: Repository, crate skeleton and event model

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `README.md` (one paragraph, phase 1 scope), `src/lib.rs`, `src/main.rs` (prints `runeway: not wired yet`, exit 0), `src/error.rs`, `src/model.rs`

**Interfaces:**
- Produces (`src/model.rs`):
  - `enum Tool { Claude, Codex, OpenCode, Gemini, Other(String) }` with `Tool::from_agent_deck(&str) -> Tool` (`"claude"`, `"codex"`, `"opencode"`, `"gemini"`, anything else → `Other`)
  - `enum SessionStatus { Running, Busy, Waiting, Idle, Ready, Stopped, Error, Unknown(String) }` with `from_agent_deck(&str)`
  - `enum ActivityKind { Thinking, Reading, Searching, Editing, RunningCommand, CommandFailed, CommandPassed, WaitingForPermission, Idle, SubagentStarted, Finished, Errored }`
  - `struct Session { id: String, title: String, path: PathBuf, tool: Tool, status: SessionStatus, tmux_session: Option<String> }`
  - `enum Event { Session(Session), SessionGone { id: String }, Activity { id: String, kind: ActivityKind, detail: String }, Usage { id: String, tokens_total: u64, context_used: Option<f32> } }`
  - `struct LaneState { session: Session, activity: ActivityKind, detail: String, tokens_total: u64, context_used: Option<f32>, transcript: Option<PathBuf>, last_change: u64 }` (`last_change` = an event counter, no clock)
  - `struct World { lanes: Vec<LaneState> }` with `fn apply(&mut self, event: Event)` and `fn waiting_count(&self) -> usize`
- Produces (`src/error.rs`): `enum Error { Io { path: PathBuf, source: io::Error }, Json { what: &'static str, source: serde_json::Error }, AgentDeck(String) }` and `type Result<T> = std::result::Result<T, Error>`.

- [ ] **Step 1:** `git init -b main` in `ai/tools/runeway` (the spec and this plan already sit in `docs/`), add `.gitignore` (`/target`), commit the docs alone as `docs: runeway design and phase 1 plan`, then `git switch -c feat/phase1-telemetry`.
- [ ] **Step 2:** Create `Cargo.toml` with `[lib]` + `[[bin]] name = "runeway"`, edition 2024, the dependencies above. Run `cargo metadata --format-version 1 | python3 -c '…max rust_version of resolved packages…'` and set `rust-version` to that value.
- [ ] **Step 3: Write the failing tests** in `src/model.rs` (`#[cfg(test)]`):
  - `session_event_adds_then_updates_lane`: applying `Event::Session` twice with the same id leaves one lane with the second status.
  - `session_gone_removes_lane`.
  - `activity_for_unknown_session_is_ignored`: no lane created.
  - `waiting_status_sets_waiting_activity`: a `Session` with `SessionStatus::Waiting` makes the lane's `activity` `WaitingForPermission`; a later `Running` status restores the last real activity.
  - `usage_updates_tokens_and_context`.
  - `waiting_count_counts_waiting_lanes`.
  - `tool_and_status_parse_from_agent_deck_strings`, including unknown strings.
- [ ] **Step 4:** Run `cargo test`. Expected: FAIL (types missing).
- [ ] **Step 5:** Implement `model.rs` and `error.rs` to pass. Lanes keep agent-deck order of first appearance.
- [ ] **Step 6:** Run the three verification commands. Expected: all pass.
- [ ] **Step 7: Commit** `feat: event model and world state`.

### Task 2: Claude transcript source

**Files:**
- Create: `src/sources/mod.rs`, `src/sources/tail.rs`, `src/sources/claude.rs`, `tests/fixtures/claude/basic.jsonl`, `tests/fixtures/claude/failed_bash.jsonl`

**Interfaces:**
- Consumes: `Event`, `ActivityKind` (Task 1).
- Produces:
  - `tail.rs`: `struct Tail { path: PathBuf, offset: u64, partial: String }`; `Tail::open_at_end(path, max_backfill: u64) -> Result<(Tail, Vec<String>)>` (returns the complete lines in the last `max_backfill` bytes, dropping a first line that starts mid-way); `Tail::read_new(&mut self) -> Result<Vec<String>>` (complete new lines only; keeps an unterminated last line in `partial`; resets to 0 if the file shrank).
  - `claude.rs`: `fn slug_for(cwd: &Path) -> String`; `fn find_transcript(projects_dir: &Path, cwd: &Path) -> Option<PathBuf>` (newest-modified `.jsonl` in `projects_dir/slug`); `struct ClaudeParser { context_window: u64, last_tool: HashMap<String, ActivityKind> }`; `fn parse_line(&mut self, session_id: &str, line: &str) -> Vec<Event>`.

Mapping rules for `parse_line` (detail = tool name plus file name or command's first word; never more):
- `Read`, `NotebookRead` → `Reading` (detail: file name of `input.file_path`)
- `Grep`, `Glob`, `ToolSearch`, `WebSearch`, `WebFetch` → `Searching`
- `Edit`, `Write`, `NotebookEdit`, `MultiEdit` → `Editing` (file name)
- `Bash` → `RunningCommand` (first word of `input.command`)
- `Agent`, `Task` → `SubagentStarted`
- any other tool (including `mcp__*`) → `Thinking` with detail = the tool name after the last `__`
- an assistant line with no tool use → `Thinking`
- a `tool_result` with `is_error: true` → `CommandFailed` (detail: the failed tool's name); without error after a `Bash` call → `CommandPassed`
- `message.usage` → `Usage { tokens_total: running sum of input + output tokens of each message, context_used: Some((input + cache_read + cache_creation) / context_window) }`
- unknown line types, invalid JSON → no events (never an error)

- [ ] **Step 1: Write the fixtures by hand** (no real transcript content): `basic.jsonl` with a Read, an Edit, a Bash call and its successful result, a usage block, and one unrelated line type; `failed_bash.jsonl` with a Bash call and an `is_error: true` result.
- [ ] **Step 2: Write the failing tests:**
  - `slug_replaces_every_non_alphanumeric`: `/home/user/.cache/tmp` → `-home-user--cache-tmp`; `/a/b-c/d_e` → `-a-b-c-d-e`.
  - `find_transcript_picks_newest_jsonl` (tempdir with two files, different mtimes via `File::set_modified`).
  - `basic_fixture_maps_to_expected_events`: exact sequence `[Reading("main.rs"), Editing("lib.rs"), RunningCommand("cargo"), CommandPassed("Bash"), Usage{..}]` (detail values per fixture).
  - `failed_bash_maps_to_command_failed`.
  - `detail_never_contains_command_arguments`: a Bash command `cargo test --secret-flag` yields detail `cargo`.
  - `invalid_and_unknown_lines_yield_nothing`.
  - `tail_returns_only_complete_lines`: write `a\nb`, read → `[a]`; append `c\n` → `[bc]`. (Review Focus 1)
  - `tail_open_at_end_skips_old_history`: a 1 MiB file opened with `max_backfill = 256 KiB` returns only lines from the last 256 KiB, none cut mid-line. (Review Focus 2)
  - `tail_resets_when_file_shrinks`.
- [ ] **Step 3:** Run `cargo test sources::`. Expected: FAIL.
- [ ] **Step 4:** Implement `tail.rs` and `claude.rs` (serde with `#[serde(default)]` loose structs or `serde_json::Value` lookups; either is fine).
- [ ] **Step 5:** Run the three verification commands. Expected: pass.
- [ ] **Step 6: Commit** `feat: follow Claude transcripts`.

### Task 3: Codex transcript source

**Files:**
- Create: `src/sources/codex.rs`, `tests/fixtures/codex/sessions/2026/11/27/rollout-2026-11-27T10-00-00-test.jsonl`

**Interfaces:**
- Consumes: `Tail` (Task 2), `Event`.
- Produces: `fn find_transcript(sessions_dir: &Path, cwd: &Path) -> Option<PathBuf>` (newest-modified rollout whose first `session_meta` line has `payload.cwd == cwd`; scans only the last 7 days of `YYYY/MM/DD` folders); `struct CodexParser { pending: HashMap<String, String> }` (call_id → command word); `fn parse_line(&mut self, session_id: &str, line: &str) -> Vec<Event>`.

Mapping: `custom_tool_call` → `RunningCommand` (detail: first word of the command found in `input`; if `input` is a JSON string containing an object, read its `cmd`/`command` field; otherwise `exec`); `custom_tool_call_output` → `CommandPassed`, or `CommandFailed` when the output reports a non-zero exit code (check `output` for an `exit_code`/`metadata.exit_code` field, or a leading `Exit code: N` line; if none can be found, `CommandPassed`); `response_item` reasoning or message → `Thinking`; `token_count` → `Usage { tokens_total: total_token_usage.total_tokens, context_used: last_token_usage.input_tokens / model_context_window }`.

- [ ] **Step 1:** Before writing the fixture, print the *key structure only* of one real `custom_tool_call.input` and `custom_tool_call_output.output` (e.g. `jq -c '.payload.input | type'` and `jq -c '.payload.output | if type=="string" then (fromjson? | keys?) else keys end'`); never print values. Record the shapes found in the ledger, then write the fixture by hand to match them.
- [ ] **Step 2: Write the failing tests:** `find_transcript_matches_cwd_and_picks_newest`, `exec_call_maps_to_running_command_word`, `nonzero_exit_maps_to_command_failed`, `token_count_maps_to_usage`, `unknown_lines_yield_nothing`.
- [ ] **Step 3:** Run `cargo test codex`. Expected: FAIL.
- [ ] **Step 4:** Implement.
- [ ] **Step 5:** Run the verification commands. Expected: pass.
- [ ] **Step 6: Commit** `feat: follow Codex rollouts`.

### Task 4: agent-deck source

**Files:**
- Create: `src/sources/agentdeck.rs`, `tests/fixtures/agentdeck/ls.json`

**Interfaces:**
- Consumes: `Session`, `Tool`, `SessionStatus`.
- Produces: `trait SessionLister { fn list(&self) -> Result<Vec<Session>>; }`; `struct AgentDeck { program: PathBuf }` implementing it by running `<program> ls --json` (no shell, stdout only, 5 s timeout); `fn parse_sessions(json: &str) -> Result<Vec<Session>>` (skips `archived: true`; empty `tmux_session` → `None`).

- [ ] **Step 1: Write the failing tests:** `parses_fixture_sessions` (three sessions: claude running, codex waiting, gemini error; one archived skipped), `invalid_json_is_an_error`, `missing_program_is_an_agentdeck_error` (program path in a tempdir that does not exist → `Error::AgentDeck` mentioning the path). (Review Focus 3)
- [ ] **Step 2:** Run `cargo test agentdeck`. Expected: FAIL.
- [ ] **Step 3:** Implement. Use `std::process::Command` with `wait_timeout` done by polling `try_wait` every 50 ms (no extra crate).
- [ ] **Step 4:** Verification commands. Expected: pass.
- [ ] **Step 5: Commit** `feat: read sessions from agent-deck`.

### Task 5: Config and collector

**Files:**
- Create: `src/config.rs`, `src/collector.rs`

**Interfaces:**
- Consumes: `SessionLister`, `claude::{find_transcript, ClaudeParser}`, `codex::{find_transcript, CodexParser}`, `Tail`, `World`.
- Produces:
  - `struct Config { claude_projects: PathBuf, codex_sessions: PathBuf, agent_deck: Option<PathBuf>, claude_context_window: u64, backfill_bytes: u64 }` with `Config::from_home(home: &Path) -> Config` (defaults: `.claude/projects`, `.codex/sessions`, `agent-deck` found on `PATH` by the binary, 200_000, 262_144).
  - `struct Collector { config: Config, world: World, followers: HashMap<String, Follower>, source_error: Option<String> }`; `fn step(&mut self, lister: &dyn SessionLister) -> Vec<Event>` (one poll: list sessions, emit `Session`/`SessionGone`, start following newly matched transcripts with `Tail::open_at_end`, read new lines, parse, apply every event to `world`, return the events); `fn world(&self) -> &World`; `fn source_error(&self) -> Option<&str>`.
  - `struct SourceReport { agent_deck: std::result::Result<usize, String>, claude_dir: bool, codex_dir: bool }` and `fn check(config: &Config, lister: &dyn SessionLister) -> SourceReport`.

- [ ] **Step 1: Write the failing tests** (tempdirs and a fake `SessionLister`; never the real home):
  - `claude_session_gets_activity_from_its_transcript`: fake lister returns one Claude session at `/w/app`; the tempdir has `projects/-w-app/x.jsonl` with a Read line; after `step`, the lane's activity is `Reading`.
  - `appended_lines_are_picked_up_on_next_step`.
  - `session_without_transcript_still_listed` (Review Focus 4): an `Other("gemini")` session has a lane with `transcript: None`.
  - `lister_error_keeps_last_world_and_sets_source_error`.
  - `removed_session_emits_session_gone`.
- [ ] **Step 2:** Run `cargo test collector`. Expected: FAIL.
- [ ] **Step 3:** Implement. Unmatched sessions are retried on every step (transcript may appear later).
- [ ] **Step 4:** Verification commands. Expected: pass.
- [ ] **Step 5: Commit** `feat: collect sessions and transcripts into one world`.

### Task 6: CLI: `status`, `status --json`, `check`

**Files:**
- Create: `src/cli.rs`, `tests/cli.rs`, `tests/fixtures/bin/fake-agent-deck` (a small shell script printing `tests/fixtures/agentdeck/ls.json`)
- Modify: `src/main.rs` (wiring only)

**Interfaces:**
- Consumes: `Config`, `Collector`, `AgentDeck`, `check`.
- Produces: clap `Cli { command: Option<Command>, lowres: bool (accepted, unused until phase 2), fps: Option<u16>, no_agent_deck: bool, claude_dir: Option<PathBuf>, codex_dir: Option<PathBuf>, agent_deck: Option<PathBuf> }`, `enum Command { Watch, Status { json: bool }, Check }`; `fn status_rows(world: &World) -> Vec<StatusRow>` and `struct StatusRow { title, tool, status, action, tokens, context_percent: Option<u8>, waiting: bool }` (`Serialize`).

Output formats:
- `status`: one aligned line per lane, e.g. `app        claude  running  editing lib.rs      12.4k tok  31%` then `N agents, M waiting`; if agent-deck failed, a first line `agent-deck unavailable: <reason>` and exit 0.
- `status --json`: `{ "agents": [StatusRow...], "waiting": M, "agent_deck_error": null | "<reason>" }`.
- `check`: one line per source with `ok` / `missing: <why>`; exit 0 only if agent-deck answered.

- [ ] **Step 1: Write the failing tests** in `tests/cli.rs` (run the built binary via `env!("CARGO_BIN_EXE_runeway")` with `--agent-deck tests/fixtures/bin/fake-agent-deck --claude-dir <tempdir> --codex-dir <tempdir>`): `status_lists_fixture_agents_and_waiting_count`, `status_json_is_valid_and_has_waiting_1`, `check_reports_sources_and_exits_0`, `missing_agent_deck_status_exits_0_with_message`, `missing_agent_deck_check_exits_1`; plus a unit test `token_counts_are_abbreviated` (999 → `999`, 12_400 → `12.4k`, 3_200_000 → `3.2M`).
- [ ] **Step 2:** Run `cargo test --test cli`. Expected: FAIL.
- [ ] **Step 3:** Implement `cli.rs` and wire `main.rs`: parse args, build `Config::from_home` with overrides, build the lister (or none with `--no-agent-deck`), run one `Collector::step`, print. `watch` (default) calls the TUI from Task 7; until then it prints the status output.
- [ ] **Step 4:** Verification commands. Expected: pass.
- [ ] **Step 5: Commit** `feat: status, status --json and check commands`.

### Task 7: TUI list view

**Files:**
- Create: `src/app.rs`, `src/view.rs`
- Modify: `src/main.rs` (`watch` runs `app::run`)

**Interfaces:**
- Consumes: `Collector`, `World`, `status_rows`.
- Produces: `view.rs` (pure, no terminal): `struct ListView { selected: usize, show_help: bool }`, `fn handle_key(&mut self, key: KeyEvent, lanes: usize) -> Action` with `enum Action { None, Quit }`; `fn render(frame: &mut Frame, area: Rect, world: &World, view: &ListView, source_error: Option<&str>)`. `app.rs`: `fn run(collector: Collector, lister: Box<dyn SessionLister + Send>) -> anyhow::Result<()>`: a `TerminalGuard` (enter raw mode + alternate screen on create, restore on drop), a panic hook that restores the terminal before printing, a source thread that calls `Collector::step` every 1 s and sends the `World` snapshot over a channel, and one loop: poll input 100 ms, receive snapshots, draw.

Screen: title bar `runeway · N agents · M waiting` (waiting count highlighted when > 0), a table with the same columns as `status`, the selected row highlighted, waiting rows marked with a gold `!`, a footer `↑↓ select · ? help · q quit`; `?` toggles a help overlay listing every key.

- [ ] **Step 1: Write the failing tests** in `view.rs`: `q_esc_and_ctrl_c_quit`, `arrows_and_jk_move_selection_within_bounds`, `question_mark_toggles_help`, and `renders_waiting_marker_and_title` using `ratatui::backend::TestBackend` (80×10) with a fixed `World` (no clocks), asserting the title line text and a `!` on the waiting row. (No full-frame snapshot.)
- [ ] **Step 2:** Run `cargo test view`. Expected: FAIL.
- [ ] **Step 3:** Implement `view.rs`, then `app.rs` and the wiring.
- [ ] **Step 4: Terminal-restore test** (Review Focus 5): a unit test that installs the panic hook with a test flag and checks `restore()` is called on panic via `std::panic::catch_unwind`, without opening a real terminal (the restore function is injected).
- [ ] **Step 5:** Verification commands. Expected: pass. Then run `cargo run` in Ghostty: the owner's real agents appear; `q`, `Esc` and `Ctrl-C` each exit cleanly with the shell intact. Record the result in the ledger.
- [ ] **Step 6: Commit** `feat: live list view`.

### Task 8: README and phase check

**Files:**
- Modify: `README.md`

- [ ] **Step 1:** Write the README: what Runeway is (one paragraph, original cast, read-only and offline), install from the repo (`cargo install --path .`), the three commands and keys, where data comes from, privacy statement, phase roadmap.
- [ ] **Step 2:** Run the verification commands plus `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` and `cargo run -- check` on the owner's machine. Expected: all pass; `check` shows agent-deck ok and both transcript folders found.
- [ ] **Step 3: Commit** `docs: runeway phase 1 readme`.
- [ ] **Step 4:** Stop for the owner: Runeway has no GitHub remote. Ask whether to merge `feat/phase1-telemetry` into the local `main`, or to create `cybercoretech/runeway` (private or public) and open a PR.
