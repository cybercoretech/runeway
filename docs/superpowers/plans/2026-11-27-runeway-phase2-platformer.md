# Runeway Phase 2: The Platformer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn Runeway's live view into the retro side-scroller: one lane per agent with an original pixel-art runner, scrolling level, items and HUD, drawn with half-block pixels; plus discovery of agents running outside agent-deck, a `--demo` mode, and CI.

**Architecture:** The library gains a pure world simulation (`world/`): an RGB pixel buffer, an original palette and sprite set, a runner state machine driven by `ActivityKind`, and a lane simulation that renders one lane into the buffer. A pure half-block packer turns the buffer into top/bottom color pairs. The binary only converts packed cells into ratatui cells, lays out lanes and HUD text, and paces frames. Discovery is a new `SessionLister` that merges agent-deck with recently active transcripts.

**Tech Stack:** Rust 1.88+, ratatui 0.30, crossterm 0.29; no new runtime dependencies.

**Spec:** `docs/superpowers/specs/2026-11-27-runeway-design.md` (sections "The platformer world", "Rendering", "Controls", "Delivery" item 2).

**Builds on (phase 1, merged first):** `model::{Session, SessionStatus, Tool, ActivityKind, Event, LaneState, World}`, `collector::Collector::{new, step, step_at, world, source_error}`, `sources::agentdeck::{SessionLister, AgentDeck, Unavailable}`, `sources::claude::{candidates, slug_for}`, `sources::codex::RolloutIndex`, `status::{status_rows, abbreviate_tokens, count_noun}`, binary `view::{ListView, Action, render}`, `app::run(collector, lister)`.

## Global Constraints

- **Original identity only.** No character, item, tile, sound, wording or layout from Nintendo or any other game: no Mario, Luigi, Goomba, Koopa, Bowser, Peach, Toad, Yoshi; no "?" blocks, green pipes, mushrooms, fire flowers, stars, flagpole/castle finish, "WORLD 1-1" or "1UP". The cast and items are the ones in the spec (rune-runner, brass automaton, hover-drone, twin-spirit, cyber-runner; rune bricks, sigil blocks, glyph coins, data gems, rune gate, beacon tower, cyber-critters). A test (Task 3) fails the build if a forbidden name appears in `src/` or `README.md`.
- Phase 1 constraints still hold: read-only, offline, labels only, no hard-coded paths in the library, no terminal code in the library, `thiserror` in the library and `anyhow` in the binary, no `unwrap`/`expect` on recoverable library paths, quit on `q`/`Esc`/`Ctrl-C` with the terminal always restored.
- Frame budget: render a full 160×48-cell world in under 4 ms in a release build on the owner's machine (measured in Task 7, not asserted in CI).
- Frame pacing: at most 30 FPS while anything animates (`--fps` overrides, 5–60), about 8 FPS when only idle loops run, no redraw when nothing changed.
- Deterministic simulation: no clocks or randomness inside `world/`; time is a tick count passed in, randomness is a seeded PRNG keyed by the session id, so snapshots are stable.
- Git: branch `feat/phase2-platformer`, rebased onto `main` after PR #1 merges; one PR to `cybercoretech/runeway`; commit format as in phase 1 (body `Project contributor`, footer, Co-Authored-By); noreply email.
- Verification on every task: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

## Review Focus

1. Many agents in a short terminal: lanes that do not fit fall back to a compact list with "+N more", and the selected agent is always visible.
2. A terminal smaller than one lane (or resized while running): no panic, a "make the window taller" notice instead of a broken frame.
3. A discovered session that agent-deck also lists: one lane, not two.
4. An agent that stops writing: its lane goes idle and is dropped from discovery after the quiet limit, without flicker.
5. `--lowres` and odd terminal heights (half-block rows need even pixel heights): no off-by-one stripe or out-of-bounds write.

Each has a test in the owning task (Tasks 6, 6, 1, 1, 2).

---

### Task 1: Discover active agents without agent-deck

**Files:**
- Modify: `src/model.rs` (add `Session::transcript: Option<PathBuf>`), `src/collector.rs` (use the hint), `src/main.rs` (wire the new lister)
- Create: `src/sources/discover.rs`

**Interfaces:**
- Produces: `struct Discovery { claude_projects: PathBuf, codex_sessions: PathBuf, active_within: Duration, codex_index: RefCell<RolloutIndex> }` implementing `SessionLister` — returns one `Session` per Claude or Codex transcript modified within `active_within` (default 10 min): `id = "claude:<file stem>"` / `"codex:<file stem>"`, `tool`, `status = Running` if modified within 60 s else `Idle`, `title` = last component of the working directory (Codex: `session_meta.cwd`; Claude: last `-`-separated part of the slug), `path` = that working directory when known, `transcript = Some(file)`.
- Produces: `struct Combined { primary: Box<dyn SessionLister + Send>, discovery: Discovery }` implementing `SessionLister`: agent-deck sessions first, then discovered ones whose transcript is not what an agent-deck session of the same tool would follow (Claude: same slug folder; Codex: same cwd). If agent-deck fails, discovered sessions are still returned and the error is kept for `source_error` (new trait method `fn last_error(&self) -> Option<String>` with a default `None`).
- `Collector`: when `session.transcript` is `Some`, that file is the only candidate.
- Config: `discover_within: Duration` (default 10 min); flag `--no-discover`.

- [ ] **Step 1: Write the failing tests** (tempdirs only): `recent_claude_transcript_becomes_a_session` (title from slug, status Running), `old_transcript_is_not_discovered`, `codex_rollout_session_uses_cwd_title`, `combined_drops_discovered_duplicate_of_agent_deck_session` (Review Focus 3), `combined_keeps_discovered_sessions_when_agent_deck_fails`, `collector_follows_the_transcript_hint`, `discovered_session_goes_idle_then_disappears` (step_at with `active_within` 10 s; mtimes set explicitly; Review Focus 4).
- [ ] **Step 2:** Run `cargo test discover`. Expected: FAIL.
- [ ] **Step 3:** Implement. Update the phase 1 `Session` literals in tests with `transcript: None`.
- [ ] **Step 4:** Verification commands; then `cargo run -- status` on the owner's machine lists this Claude session. Record in the ledger.
- [ ] **Step 5: Commit** `feat: show agents running outside agent-deck`.

### Task 2: Pixel buffer, palette and half-block packer

**Files:**
- Create: `src/world/mod.rs`, `src/world/pixels.rs`, `src/world/palette.rs`, `src/world/halfblock.rs`

**Interfaces:**
- Produces: `type Rgb = [u8; 3]`; `struct Pixels { width: usize, height: usize, data: Vec<Rgb> }` with `new(width, height, fill: Rgb)`, `set(x: i32, y: i32, c: Rgb)` (ignores out-of-bounds), `get(x, y) -> Rgb`, `fill_rect(x, y, w, h, c)`, `blit(sprite: &Sprite, x: i32, y: i32, flip: bool)` (palette index 0 = transparent).
- Produces: `palette::PALETTE: [Rgb; 16]` (original Cybercore-tuned colors: night sky, deep panel, two skyline shades, ground, ground shadow, rune cyan, rune violet, ember pink, glyph gold, gem green, brass, steel, white, warning red, shadow) and named indices (`SKY`, `GLYPH_GOLD`, …).
- Produces: `struct HalfCell { top: Rgb, bottom: Rgb }`; `fn pack(px: &Pixels) -> Vec<Vec<HalfCell>>` (rows = ceil(height / 2); an odd last row repeats the background as bottom); `fn pack_lowres(px: &Pixels) -> Vec<Vec<Rgb>>` (one pixel per cell: average of each vertical pair).

- [ ] **Step 1: Write the failing tests:** `set_ignores_out_of_bounds`, `blit_skips_transparent_and_flips`, `pack_pairs_rows_into_cells`, `pack_odd_height_has_no_stripe` (Review Focus 5), `lowres_averages_pairs`, `palette_has_16_distinct_colors`.
- [ ] **Step 2:** `cargo test world::`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Verification commands.
- [ ] **Step 5: Commit** `feat: pixel buffer, palette and half-block packer`.

### Task 3: The original cast, tiles and items

**Files:**
- Create: `src/world/sprites.rs`, `tests/originality.rs`

**Interfaces:**
- Produces: `struct Sprite { width: usize, height: usize, pixels: Vec<u8> }` built from `const` text maps by `Sprite::from_rows(rows: &[&str], key: &[(char, u8)]) -> Sprite` (`.` = transparent); `enum Character { RuneRunner, BrassAutomaton, HoverDrone, TwinSpirit, CyberRunner }` with `Character::for_tool(&Tool) -> Character`; `fn frames(character, pose: Pose) -> &'static [Sprite]` where `enum Pose { Run, Jump, Stumble, Sit, Stand, Cheer }`; tiles and items: `Tile::{Ground, RuneBrick, SigilBlock, Gate, Beacon}`, `Item::{GlyphCoin, DataGem, Spark}`, `Critter` (cosmetic patroller), and a parallax `Backdrop::{Skyline, ServerRacks, RunePillars}` strip.
- Sizes: characters 16×16 (hover-drone 16×12), tiles 16×16, items 8×8. 2–4 run frames, 1 frame for other poses.

Design brief for the sprites (original, Cybercore-flavored; checked by the owner before merge): the rune-runner is a hooded figure with a cyan visor slit and a trailing cloak; the brass automaton is a round-bodied walker with a small chimney that puffs on run frames; the hover-drone is a lens with two side rotors; the twin-spirit is two linked orbs of violet and cyan; the cyber-runner is a plain visored runner tinted per agent name. Rune bricks carry a glowing glyph cut into the brick, sigil blocks a ring sigil, the gate is a portcullis of light bars, the beacon a tall pylon with a pulsing tip.

- [ ] **Step 1: Write the failing tests:** `every_sprite_row_has_its_width`, `every_sprite_uses_known_palette_keys`, `every_character_has_all_poses`, `for_tool_maps_each_agent`, and `tests/originality.rs::no_borrowed_game_names` (scans every `.rs` file under `src/` and `README.md`, case-insensitive, for: mario, luigi, goomba, koopa, bowser, peach, toad, yoshi, 1-1, 1up, mushroom, fire flower, warp pipe, question block, flagpole; any hit fails with file and line).
- [ ] **Step 2:** `cargo test sprites originality`. Expected: FAIL.
- [ ] **Step 3:** Draw the sprites as text maps and implement.
- [ ] **Step 4:** Write `examples/sprite_sheet.rs` that renders every sprite at 4× into the terminal with half-blocks (for the owner's visual check); run it once and record that it renders.
- [ ] **Step 5:** Verification commands.
- [ ] **Step 6: Commit** `feat: original cast, tiles and items`.

### Task 4: Runner states and the activity mapping

**Files:**
- Create: `src/world/runner.rs`, `src/world/mapping.rs`

**Interfaces:**
- Produces: `enum RunnerState { Running, Collecting, Smashing, MachineSection, Stumbling, Celebrating, AtGate, Idle, Finished, LostSignal }`; `fn state_for(kind: ActivityKind, status: &SessionStatus) -> RunnerState` (spec table; `Error`/`Stopped` status → `LostSignal`; `Waiting` → `AtGate`; `SubagentStarted` → `Celebrating`); `struct Runner { state, pose: Pose, frame: usize, x: i32, y: i32, vy: i32, state_ticks: u32 }` with `fn tick(&mut self)` (gravity, a jump arc for `Smashing`, knock-back for `Stumbling`, which last a fixed number of ticks before returning to `Running`) and `fn set_state(&mut self, state)`; `fn scroll_speed(state) -> i32` (0 at the gate, idle, finished or lost signal).

- [ ] **Step 1: Write the failing tests:** one test per spec row of the mapping table; `stumble_returns_to_running_after_its_ticks`; `smash_jump_lands_back_on_the_ground`; `gate_and_idle_do_not_scroll`.
- [ ] **Step 2:** `cargo test runner mapping`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Verification commands.
- [ ] **Step 5: Commit** `feat: runner states driven by agent activity`.

### Task 5: Lane simulation and rendering

**Files:**
- Create: `src/world/lane.rs`, `src/world/rng.rs`, `tests/snapshots/lane_claude_tick40.txt`

**Interfaces:**
- Consumes: `Pixels`, sprites, `Runner`, `state_for`.
- Produces: `struct Rng(u64)` (xorshift, `Rng::for_id(&str)`); `struct LaneSim { id, character, runner, scroll: i32, level: Level, effects: Vec<Effect>, last_activity_change: u64 }`; `LaneSim::new(lane: &LaneState) -> LaneSim`; `fn sync(&mut self, lane: &LaneState)` (applies a new activity: switches runner state, spawns coins when reading, a brick + gem when editing, sparks when a command passes); `fn tick(&mut self)`; `fn render(&self, px: &mut Pixels)` (backdrop with parallax, ground, level tiles, items, runner, gate and glyph when `AtGate`, beacon when finished, grey "lost signal" overlay). `struct Level` generates tiles in 64-px chunks from the seeded `Rng`, keeps only chunks on screen.
- Snapshot format: the rendered buffer as text, one character per pixel from the palette index (`0-9a-f`), so a diff shows which pixels changed.

- [ ] **Step 1: Write the failing tests:** `same_id_same_level` (determinism), `reading_spawns_coins`, `editing_spawns_brick_and_gem`, `waiting_shows_gate_and_stops_scroll`, `finished_shows_beacon`, `lost_signal_greys_lane`, and `lane_snapshot_is_stable` (a Claude lane, editing, rendered at tick 40 into 160×22 px, compared with `tests/snapshots/lane_claude_tick40.txt`; `RUNEWAY_UPDATE_SNAPSHOTS=1` rewrites it).
- [ ] **Step 2:** `cargo test lane`. Expected: FAIL.
- [ ] **Step 3:** Implement; generate the snapshot once, look at it with the sprite-sheet example, commit it.
- [ ] **Step 4:** Verification commands.
- [ ] **Step 5: Commit** `feat: lane simulation and rendering`.

### Task 6: World view in the terminal

**Files:**
- Create: `src/world_view.rs` (binary)
- Modify: `src/view.rs` (shared header/footer, `l` toggles list), `src/app.rs`, `src/cli.rs` (`--lowres`, `--fps` now used)

**Interfaces:**
- Consumes: `LaneSim`, `pack`, `pack_lowres`, `status_rows`, `abbreviate_tokens`.
- Produces: `struct WorldView { sims: HashMap<String, LaneSim>, mode: ViewMode, lowres: bool }`, `enum ViewMode { World, List }`; `fn sync(&mut self, world: &World)`; `fn tick(&mut self)`; `fn render(&self, frame, area, world, view: &ListView, source_error)`. Layout: top bar (`runeway · N agents · M waiting · Σ tokens`), then lanes of 12 rows each (1 HUD text row: name, tool, glyph-coin token count, a context meter `▰▰▰▱▱`, action label; then 11 rows = 22 half-block px of world), selected lane outlined; lanes that do not fit become one compact line each with "+N more" (Review Focus 1); a terminal shorter than one lane shows "make the window taller" (Review Focus 2); footer `↑↓ select · l list · ? help · q quit`.
- Writes packed cells into the ratatui buffer: symbol `▀`, `fg` = top, `bg` = bottom (`Color::Rgb`); lowres writes `' '` with `bg`.

- [ ] **Step 1: Write the failing tests** with `TestBackend`: `top_bar_counts_and_tokens`, `hud_row_shows_name_tokens_meter_and_action`, `waiting_lane_border_is_gold`, `more_lanes_than_fit_fall_back_with_more_marker`, `tiny_terminal_shows_notice_not_panic` (sizes 20×5, 40×8, resize sequence), `l_toggles_list_view`, `lowres_uses_one_pixel_per_cell`.
- [ ] **Step 2:** `cargo test --bin runeway world_view`. Expected: FAIL.
- [ ] **Step 3:** Implement; the world view becomes the default, the phase 1 list stays behind `l`.
- [ ] **Step 4:** Verification commands.
- [ ] **Step 5: Commit** `feat: the platformer world view`.

### Task 7: Frame pacing, demo mode, benchmark and CI

**Files:**
- Modify: `src/app.rs`, `src/cli.rs`, `src/main.rs`, `README.md`
- Create: `src/demo.rs` (library), `examples/frame_bench.rs`, `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `demo::world_at(tick: u64) -> World` (deterministic script: four original agents — a rune-runner reading then editing then passing a check, an automaton running a command that fails then passes, a drone waiting at the gate for a while, a twin-spirit finishing at the beacon — looping every 60 s at 30 ticks/s); `--demo` runs the live view from `world_at` instead of the collector.
- Pacing in `app.rs`: an animation clock (`Instant` at 30 ticks/s); draw when the world snapshot changed, when any lane is animating (`WorldView::animating()`), or on input; otherwise sleep until the next 8 FPS idle tick; `--fps N` caps the animating rate (clamped 5–60).
- `examples/frame_bench.rs`: build a 12-lane demo world, render 300 frames at 160×48 cells into a `TestBackend` buffer, print mean and p95 ms.

- [ ] **Step 1: Write the failing tests:** `demo_world_has_four_agents_and_one_waiting_at_some_tick`, `demo_loops`, `pacing_draws_only_when_needed` (a pure `fn should_draw(changed, animating, input, since_last, fps) -> bool`), `fps_flag_is_clamped`.
- [ ] **Step 2:** `cargo test demo pacing`. Expected: FAIL.
- [ ] **Step 3:** Implement; add CI (`ubuntu-latest`, stable toolchain plus 1.88 for `cargo check`: fmt, clippy `-D warnings`, test, doc).
- [ ] **Step 4:** Run `cargo run --release --example frame_bench`; record mean and p95 in the ledger; if p95 ≥ 4 ms, profile and fix before continuing.
- [ ] **Step 5:** Update the README: the world view, keys (`l`, `--demo`, `--lowres`, `--fps`), discovery, the cast table, CI badge; keep the privacy section.
- [ ] **Step 6:** Verification commands, then the owner's visual check in Ghostty: `cargo run --release -- --demo` and `cargo run --release` with real agents. Record the owner's verdict in the ledger.
- [ ] **Step 7: Commit** `feat: frame pacing, demo mode, benchmark and CI`; push and open the PR after PR #1 is merged and the branch is rebased.
