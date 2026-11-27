# Runeway Phase 3: Full Mapping and Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the spec's phase 3. Discovered Claude sessions get their live status (including waiting) from Claude's own session records. OpenCode agents appear, read from OpenCode's SQLite database. `Enter` jumps to the selected agent: its tmux session, or the Ghostty window running it. `p` pauses. The README gets real screenshots.

**Architecture:**
- New library sources:
  - `sources::claude_sessions` reads `~/.claude/sessions/<pid>.json` (fields `pid`, `sessionId`, `status` only; never the `.key` files).
  - `sources::opencode` reads `opencode.db` read-only.
- A pure `jump` module turns a lane into a jump plan from a fake-able `/proc` and a list of window PIDs. The binary only runs `tmux` / `hyprctl` and shows a one-line notice.
- Screenshots are SVGs rendered from a `TestBackend` frame of the demo, so they are reproducible and need no font.

**Tech Stack:** Rust 1.89+ (raised from 1.88: `rusqlite` 0.43 needs it), ratatui 0.30, crossterm 0.29, `rusqlite` 0.43 with `bundled` (the first new runtime dependency).

**Spec:** `docs/superpowers/specs/2026-11-27-runeway-design.md`: "Controls" (`Enter`, `p`), "Delivery" item 3. Owner said "dive in" on 2026-11-27. Decisions taken for the owner:
- `Enter` on a tmux-backed agent-deck session uses tmux (`switch-client` inside tmux, else show the attach command). Any other agent focuses its terminal window through Hyprland.
- The process fallback is dropped: discovery already covers it, and a process scan finds non-agent helpers (Claude's Chrome bridge, background Codex).

## Global Constraints

- Phase 1–2 constraints hold: read-only and offline for agent data, labels only (never file contents, prompts or full commands), no hard-coded paths in the library (every location comes from `Config`), no terminal code in the library, `thiserror` in the library and `anyhow` in the binary, no `unwrap`/`expect` on recoverable library paths, original identity only (`tests/originality.rs`).
- Opening OpenCode's database: `SQLITE_OPEN_READ_ONLY`, `busy_timeout` 200 ms. Never read the tables `credential`, `account`, `control_account` or `kv`.
- Never read `~/.claude/sessions/*.key`.
- External commands (`tmux`, `hyprctl`) run only on an explicit `Enter`, without a shell. Their stdout/stderr is captured, never printed into the TUI.
- `rust-version = "1.89"`; the CI MSRV job checks 1.89.
- Git: branch `feat/phase3-polish`, based on `fix/discovery-polish` until that PR merges, then rebased onto `main`. One PR. Commit format as in phases 1–2.
- Verification on every task: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

## Review Focus

1. OpenCode's database missing, locked by a writer, or with a different schema: no panic, no OpenCode lanes, `check` says why.
2. `Enter` outside Hyprland, with `hyprctl` or `tmux` missing, or when no window is found: a one-line notice, no crash, and no command output bleeding into the screen.
3. Two agents of the same tool in the same folder: the Claude jump uses the exact PID from Claude's session record. Codex and OpenCode refuse to guess and say so.
4. A stale Claude session record (its PID is gone) or an unknown `status` value: ignored, with the mtime-based status still used.
5. Pausing: no ticks while paused, input still works, and unpausing does not burst through the missed ticks.

Each has a test in its owning task (Tasks 2, 3, 3, 1, 4).

---

### Task 1: Live status for discovered Claude sessions

**Files:**
- Create: `src/sources/claude_sessions.rs`
- Modify: `src/model.rs` (`Session::pid: Option<u32>`), `src/config.rs` (`claude_sessions: PathBuf` = `~/.claude/sessions`, `proc_root: PathBuf` = `/proc`), `src/sources/discover.rs`, every `Session` literal (`pid: None`)

**Interfaces:**
- Produces: `struct ClaudeSessionRecord { pid: u32, session_id: String, status: Option<String> }`
- Produces: `fn read_records(dir: &Path, proc_root: &Path) -> HashMap<String, ClaudeSessionRecord>`, keyed by `sessionId`.
  - Reads only `*.json` files.
  - Skips a record whose `proc_root/<pid>` does not exist.
- Produces: `fn status_from(record: &str) -> Option<SessionStatus>`:
  - `"busy"` → `Running`
  - `"idle"` → `Idle`
  - any value containing `wait` or `permission` → `Waiting`
  - anything else → `None` (keep the mtime status)
- Discovery: a Claude transcript whose stem matches a record uses that record's status when `status_from` gives one, and sets `Session::pid`.

- [ ] **Step 1: Write the failing tests** (tempdirs and a fake proc root only):
  - `records_map_session_ids_to_pids`
  - `stale_record_is_ignored` (no `proc/<pid>` dir)
  - `key_files_are_never_read` (a `.key` file with invalid JSON next to a valid record: still one record)
  - `status_values_map` (`busy`, `idle`, `waiting_for_permission`, `mystery` → `None`)
  - `discovered_claude_session_takes_record_status_and_pid` (record `waiting` → `SessionStatus::Waiting`, `pid` set)
- [ ] **Step 2:** `cargo test claude_sessions discover`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Verification commands; `cargo run -- status` on the owner's machine still lists this session as running. Record in the ledger.
- [ ] **Step 5: Commit** `feat: live status for discovered Claude sessions`.

### Task 2: OpenCode agents

**Files:**
- Create: `src/sources/opencode.rs`, `tests/fixtures/opencode/schema.sql` (minimal subset of `session_v2` and `session_message` columns used)
- Modify: `Cargo.toml` (`rusqlite = { version = "0.43", features = ["bundled"] }`, `rust-version = "1.89"`), `src/config.rs` (`opencode_db: PathBuf` = `~/.local/share/opencode/opencode.db`), `src/sources/discover.rs` (`Discovery::opencode: Option<OpenCodeDb>`), `src/collector.rs` (poll OpenCode lanes), `src/collector.rs::check` + `main.rs` check output (`opencode` line), `src/cli.rs` (`--opencode-db PATH`), `.github/workflows/ci.yml` (MSRV 1.89)

**Interfaces:**
- Produces: `struct OpenCodeDb { path: PathBuf }`.
- Produces: `fn sessions(&self, now: SystemTime, active_within: Duration) -> Result<Vec<Session>>`. It lists top-level sessions (`parent_id IS NULL`, `time_archived IS NULL`) with `time_updated` (ms) within `active_within`. Each becomes:
  - `id = "opencode:<id>"`, `tool = Tool::OpenCode`
  - `title` = last component of `directory`, `path = directory`
  - `status = Running` if `time_busy > coalesce(time_idle, 0)`, else `Idle`
- Produces: `fn latest(&self, session_id: &str) -> Result<Option<(ActivityKind, String, u64)>>`: the newest assistant message's last `tool` part mapped to (kind, label), and tokens = `tokens_input + tokens_output + tokens_reasoning` from `session_v2`.
  - `read` → Reading; `glob` / `grep` / `list` / `webfetch` / `websearch` → Searching
  - `edit` / `write` / `patch` → Editing
  - `shell` / `bash`: state `running` → RunningCommand, `completed` → CommandPassed, `error` → CommandFailed
  - `question` with state `running` → WaitingForPermission
  - no tool part → Thinking
  - label: file basename from `input.filePath`, the first word of `input.command` (reuse `claude::first_word`), or empty
- `Collector`: for lanes with `tool == OpenCode` and `id` starting `opencode:`, it calls `latest` each step and emits `Activity`/`Usage` only when the value changed.
- Errors: a missing file → no OpenCode sessions, silently. Open or query failure → `Error::OpenCode(String)` (new variant), reported by `check` and as the source error.

- [ ] **Step 1: Write the failing tests**. Each builds a temp DB from `schema.sql` with rusqlite.
  - `recent_top_level_session_is_listed` (title, path, Running)
  - `child_and_archived_sessions_are_skipped`
  - `old_session_is_not_listed`
  - `latest_maps_tool_parts` (one case per mapping row above)
  - `missing_db_lists_nothing`
  - `wrong_schema_is_an_error_not_a_panic` (DB with no `session_v2`)
  - `locked_db_does_not_hang` (another connection holds `BEGIN EXCLUSIVE`; `sessions` returns within 1 s with an error)
  - `collector_emits_opencode_activity_once` (two steps, same DB state → one Activity event)
  - `check_reports_opencode`
- [ ] **Step 2:** `cargo test opencode`. Expected: FAIL.
- [ ] **Step 3:** Implement. Open per query (`OpenFlags::SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX`), so WAL readers see fresh data.
- [ ] **Step 4:** Verification commands; `cargo run -- status` against the owner's real `opencode.db` (read-only). Record the result.
- [ ] **Step 5: Commit** `feat: OpenCode agents from its database`.

### Task 3: Jump to the agent

**Files:**
- Create: `src/jump.rs` (library, pure), `src/jumper.rs` (binary: runs commands)
- Modify: `src/view.rs` (`Enter` → `Action::Jump`), `src/world_view.rs` (one-line notice in the footer for 3 s), `src/app.rs`

**Interfaces:**
- Produces: `enum JumpPlan { TmuxSwitch(String), TmuxAttachHint(String), FocusWindow(u32), NotFound(String) }`
- Produces: `fn plan(session: &Session, in_tmux: bool, proc_root: &Path, window_pids: &[u32]) -> JumpPlan`:
  - if `tmux_session` is `Some`: `TmuxSwitch` when `in_tmux`, else `TmuxAttachHint("tmux attach -t <name>")`
  - else the agent PID: `session.pid`, or the single process under `proc_root` whose `comm` is `codex` / `opencode` / `opencode.exe` and whose `cwd` symlink equals `session.path`
  - then walk parents (`stat` field 4) to the first PID in `window_pids` → `FocusWindow(pid)`
  - several candidates → `NotFound("2 codex agents in <dir>; can't tell which")`
  - none → `NotFound("no window found for <title>")`
- Binary `jumper::run(plan) -> String`, the notice text:
  - `tmux switch-client -t <name>`
  - `hyprctl dispatch focuswindow pid:<pid>`
  - window PIDs come from `hyprctl clients -j`, field `pid`
  - when Hyprland is not running, `window_pids` is empty
  - commands use `std::process::Command` with captured output, and a non-zero exit becomes the notice

- [ ] **Step 1: Write the failing tests** (fake proc root in a tempdir: `<pid>/stat`, `<pid>/comm`, `<pid>/cwd` symlink):
  - `tmux_session_inside_tmux_switches`, `tmux_session_outside_tmux_gives_attach_hint`
  - `claude_pid_walks_up_to_its_window`
  - `codex_found_by_cwd_walks_up_to_its_window`
  - `two_codex_in_one_folder_refuses_to_guess`
  - `no_window_ancestor_is_not_found`
  - `parent_loop_or_missing_stat_does_not_hang` (stat whose ppid is itself)
  - binary: `enter_returns_jump_action`, `notice_shows_in_footer_then_expires`
- [ ] **Step 2:** `cargo test jump`. Expected: FAIL.
- [ ] **Step 3:** Implement; the `?` help lists `Enter`.
- [ ] **Step 4:** Verification commands. On the owner's machine, run the binary in a pty, select this session's lane, press `Enter`, and confirm that the notice names a window (or explains why not). Record the result.
- [ ] **Step 5: Commit** `feat: Enter jumps to the agent's tmux session or window`.

### Task 4: Pause

**Files:** Modify `src/view.rs` (`p` → `Action::TogglePause`), `src/world_view.rs` (`paused: bool`; `tick` does nothing while paused; footer shows `paused`), `src/app.rs` (on unpause, move the animation clock forward so missed ticks are skipped)

- [ ] **Step 1: Write the failing tests:**
  - `p_toggles_pause`
  - `paused_world_does_not_tick` (scroll unchanged after ticks)
  - `paused_footer_says_paused`
  - `unpause_skips_missed_ticks` (a pure `fn ticks_due(elapsed_ticks, done, paused_ticks) -> u64` in `app.rs`)
- [ ] **Step 2:** `cargo test --bin runeway pause`. Expected: FAIL.
- [ ] **Step 3:** Implement; `?` help lists `p`.
- [ ] **Step 4:** Verification commands.
- [ ] **Step 5: Commit** `feat: p pauses the animation`.

### Task 5: Screenshots and README

**Files:** Create `examples/screenshot.rs`, `docs/screenshots/world.svg`, `docs/screenshots/list.svg`. Modify `README.md`, `tests/originality.rs` (also scan `examples/`; match `1-1` only as a whole word so dates do not trip it).

- `examples/screenshot.rs`: draw the 4-agent demo at tick 450 into a 120×52 `TestBackend` (world view, and list view after `l`). Write an SVG:
  - each cell is a 9×18 px `rect`; `▀` cells get two 9×9 rects (fg top, bg bottom)
  - other symbols become a `<text>` in `monospace` with their fg colour
  - default colours are `#0a0818` bg and `#ecf0ff` fg
- README: the two screenshots at the top, keys `Enter` and `p`, OpenCode in the sources table, the Claude session-record status, and `check`'s new line.

- [ ] **Step 1: Write the failing tests** in `tests/originality.rs`:
  - `matcher_ignores_dates_but_flags_level_names`: `hits("2021-11-05")` is empty and `hits("level 1-1")` is not, where `fn hits(line: &str) -> Vec<&'static str>` is the matcher pulled out of the scan.
  - `examples_are_scanned`: the scanned file list includes `examples/sprite_sheet.rs`.
- [ ] **Step 2:** `cargo test --test originality`. Expected: FAIL.
- [ ] **Step 3:** Implement the matcher change, the example, generate the SVGs, update the README.
- [ ] **Step 4:** Look at both SVGs (convert to PNG in the scratchpad); verification commands.
- [ ] **Step 5: Commit** `docs: screenshots and phase 3 README`.
