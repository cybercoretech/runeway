# Runeway: design

Status: approved by owner (2026-11-27)

## Why

The owner runs many coding agents at once (Claude Code, Codex, OpenCode, Gemini and others, mostly through agent-deck in tmux) and cannot see at a glance which one is working, which one is stuck, and which one is waiting for a permission answer. Runeway is a terminal app that shows every running agent as a character running through its own side-scrolling level, in the spirit of the 8-bit platformers the owner grew up with (Super Mario Bros., 1985). It should be useful (status, cost, attention needed) and a joy to leave open on a second screen.

## Goals

- One lane per agent session, stacked vertically, readable at a glance: what each agent is doing right now, how much it has spent, how much context is left, and whether it needs the owner.
- A 1985-era platformer feel: chunky pixels, a limited bright palette, tile scrolling, a character that runs, jumps and bumps blocks, a score/coin counter, and a goal at the end of the level.
- Works today with the owner's setup and touches nothing: reads agent-deck's session list and the agents' own transcripts.
- Smooth in Ghostty; still usable over SSH.
- Jump from a lane to that agent's tmux pane.

## Non-goals (for now)

- Pushing events from agent hooks (approach B). It can be added later behind the same event model.
- Controlling agents (sending prompts, answering permission prompts) from Runeway.
- Sound.
- The Doom/Hexen dungeon theme. The renderer is built so a second theme can be added later, but only the platformer theme is designed here.
- Publishing. The repo stays local until the owner decides.

## Anti-goals (failure even if it works)

- **Anything that copies Nintendo's property.** No Mario, Luigi, Goombas, Koopas, Bowser, Princess, Toad; no question-mark blocks, green warp pipes, super mushrooms, fire flowers, stars, the flagpole or castle finish, the "WORLD 1-1" HUD wording, Nintendo's sprites, tiles, level layouts, music, sound effects or fonts; no names, marks or near-copies of any of these. The genre (running, jumping, scrolling, collecting, a goal at the end, a limited bright palette) is free to use; every character, tile, item and HUD element is original.
- Slowing down, blocking or changing any agent. Runeway is strictly read-only.
- Writing to, copying or sending anywhere the transcripts it reads (they contain code, paths and sometimes secrets). No network access at all.
- A dashboard that looks fun but lies: every on-screen state must come from real telemetry.

## Approaches considered

| | How Runeway learns what agents do | Trade-off |
|---|---|---|
| A. Read session files | Watch each agent's transcript files | Zero setup, read-only; about 1 s of lag, one parser per agent format |
| B. Hooks push events | Agent hooks write events to a local socket | Instant; but changes every agent's config and covers only hookable agents |
| **C. A + agent-deck (chosen)** | Transcripts for activity; `agent-deck ls --json` for which sessions exist, their status and tmux session | No agent config touched, reuses the owner's session list, and gives tmux attach for free; depends on agent-deck for the session list, with a process-scan fallback |

The owner chose C.

## Data sources

| Source | What it gives | How |
|---|---|---|
| `agent-deck ls --json` | Sessions: `id`, `title`, `path`, `tool`, `status` (`running`, `busy`, `waiting`, `idle`, `ready`, `stopped`, `error`), `substate`, `tmux_session`, `last_activity_at` | Poll every 2 s; the session list is the lanes. `waiting` is the only source for "waiting for permission": transcripts do not record it |
| Claude Code transcripts `~/.claude/projects/<path-slug>/<session-id>.jsonl` | Each assistant message's `tool_use` blocks (tool name and input), tool results (including errors and Bash exit status), and `message.usage` (input, cache and output tokens) | Tail the newest file for the session's path; parse appended lines only |
| Codex transcripts `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | `response_item` `custom_tool_call` (`exec`) and its output, `event_msg` `token_count`, reasoning items | Same tailing approach |
| OpenCode `~/.local/share/opencode/opencode.db` (SQLite, WAL) | Messages and tool parts | Phase 3: open read-only (`mode=ro`), poll; until then OpenCode lanes run on agent-deck status only |
| Process table (fallback) | Agent processes not managed by agent-deck; child commands (`cargo`, `git`, `rg`) | Optional scan for unmanaged agents and for "running a command" detail |

Mapping a session to its transcript: Claude uses the session's working directory to find `~/.claude/projects/<slug>` and takes the most recently modified `.jsonl` there; Codex matches by working directory and start time recorded in the rollout's first lines. If no transcript can be matched, the lane runs on agent-deck status alone and says so in its HUD.

## Event model

Every source produces the same events, so renderers never see agent-specific formats:

- `Session { id, title, tool, status, tmux_session }` (from agent-deck)
- `Activity { id, kind, detail }` where `kind` is one of `Thinking`, `Reading`, `Searching`, `Editing`, `RunningCommand`, `CommandFailed`, `CommandPassed`, `WaitingForPermission`, `Idle`, `SubagentStarted`, `Finished`, `Errored`
- `Usage { id, tokens_total, context_used_fraction }`

`detail` is a short safe label (tool name, file name, command name). It never carries file contents or full command lines.

## The platformer world (all original)

**Lanes.** Each lane is a strip of level: sky, a scrolling parallax background (Cybercore skyline, server racks, rune pillars per theme), a ground row of tiles, and the agent's runner. Lanes are 12 terminal rows tall (24 half-block pixels). The selected lane is outlined.

**Cast.** One original runner per agent family, drawn as 16×16 (or 16×24) pixel sprites with 3–4 colors each:
- Claude: a hooded rune-runner with a glowing visor
- Codex: a small brass automaton with a chimney puff
- OpenCode: a hovering drone with a propeller
- Gemini: a twin-spirit (two small linked orbs)
- Any other agent: a generic cyber-runner, tinted by name

**Original items and tiles.** Rune bricks (breakable), sigil blocks (bump to release a glyph), data gems, coins shaped as glowing glyphs, a locked rune gate, a beacon tower as the level goal, spark bursts, small cyber-critters that patrol (cosmetic only).

**State mapping.**

| Agent state | Runner does |
|---|---|
| Thinking | Runs forward; the level scrolls |
| Reading / searching files | Runs and collects glyph coins (one per file read) |
| Editing / writing | Jumps and breaks a rune brick; a data gem pops out |
| Running a command | Enters a machine section: gears turn, steam vents |
| Command failed (non-zero exit, failed check) | Knocked back, red flash, a brief stumble |
| Command passed | Spark shower |
| Waiting for permission | Stops at a locked rune gate; a gold glyph pulses above; the lane border pulses |
| Sub-agent started | A small companion pops out and follows |
| Idle (no activity for a while) | Sits on a block, tapping its foot |
| Finished | Reaches the beacon tower; fanfare animation; lane dims |
| Error / exited (agent-deck status error) | Lane greys out with a "lost signal" overlay |

**HUD per lane** (top line of the lane, in an original pixel font): agent name and tool, glyph-coin counter (tokens used, abbreviated: 12.4k), a context meter that drains as context fills, and the current action label (`editing main.rs`). A global top bar shows the total coins (tokens) across all agents and the count of agents waiting for the owner.

**Palette.** An original limited palette (about 16 colors) tuned for the Cybercore look, bright on dark; optionally a palette per lane theme.

## Rendering

- `ratatui` + `crossterm`, full-screen alternate buffer.
- Each lane renders into an off-screen RGB pixel buffer; the half-block renderer packs two pixels per cell with `▀`, top pixel as foreground and bottom as background (`Color::Rgb`).
- Frame budget: 30 FPS while anything animates, about 8 FPS when only idle loops run, no redraw when nothing changed. A `--lowres` mode (one pixel per cell, fewer colors) for SSH.
- Sprites and tiles live in source as small pixel maps with a palette index per pixel, so there is no asset loading and they are easy to tweak; an optional PNG import can come later.
- Terminal resize recomputes lane count and scale; lanes that do not fit become a compact one-line list with a "+N more" marker.

## Controls

| Key | Action |
|---|---|
| `↑` `↓` / `k` `j` | Select lane |
| `Enter` | Jump to the agent's tmux session (`tmux switch-client` inside tmux, otherwise print the attach command) |
| `p` | Pause animations |
| `l` | Toggle the plain list view (the Phase 1 view) |
| `?` | Help overlay listing every key |
| `q`, `Esc`, `Ctrl-C` | Quit; the terminal is always restored, including after a panic (panic hook + drop guard) |

## Command line

The TUI is one surface; the same information is available to scripts and agents:

| Command | Does |
|---|---|
| `runeway` / `runeway watch` | The full-screen world (default) |
| `runeway status` | One line per agent: tool, status, action, tokens, context %; `--json` for scripts |
| `runeway check` | Reports which sources were found (agent-deck, Claude, Codex, OpenCode) and why any is missing |

Flags: `--lowres`, `--fps <n>`, `--no-agent-deck`, and path overrides (`--claude-dir`, `--codex-dir`, `--opencode-db`) that default to the standard locations under `$HOME` / XDG dirs.

## Architecture

One crate with a library and a thin binary (`[lib]` + `[[bin]]`). The library holds the event model, the parsers, the mapping and the world simulation, and never touches the terminal; the binary wires the CLI, the sources and the TUI. Source locations come from config and flags (defaulting from `$HOME`), never from constants, so every domain test runs on fixture directories instead of the real home folder. Errors: `thiserror` in the library, `anyhow` at the binary edge, no `unwrap` on recoverable paths.

```text
src/
  lib.rs             re-exports; no terminal code
  main.rs            wiring only: parse args, load config, run
  cli.rs             clap: watch | status [--json] | check, flags
  config.rs          resolved source paths and options
  error.rs           library error type
  app.rs             TUI event loop: one loop for tick, input, render
  model.rs           Session, Activity, Usage, per-lane view model
  sources/
    agentdeck.rs     poll `agent-deck ls --json`
    claude.rs        tail + parse Claude transcripts
    codex.rs         tail + parse Codex rollouts
    opencode.rs      read-only SQLite (phase 3)
    process.rs       fallback scan (sysinfo)
  mapping.rs         events -> runner animation states (pure)
  world/
    level.rs         procedural tiles per lane, scrolling, items
    sprites.rs       original sprite + tile pixel maps, palette
    runner.rs        runner physics and animation frames
  render/
    halfblock.rs     RGB buffer -> terminal cells
    lanes.rs         lane layout, HUD, top bar
tests/fixtures/      small, redacted transcript samples per agent
```

Sources run on a background thread and only send events over a channel; drawing happens in the one event loop, which never blocks on I/O.

## Delivery

One PR at a time, each merged before the next:

1. **Telemetry + list view.** agent-deck source, Claude and Codex parsers, the event model and a plain ratatui list (one row per agent: tool, action, tokens, context %, waiting flag). Useful on its own.
2. **Platformer renderer.** Half-block renderer, lanes, the original cast and tiles, HUD, frame pacing, `--lowres`. Moved forward from phase 3 at the owner's request (2026-11-27), because most of the owner's agents run outside agent-deck: discovery of Claude and Codex sessions that are actively writing a transcript, and a `--demo` mode that plays scripted lanes. CI (GitHub Actions running fmt, clippy, tests).
3. **Full mapping + polish.** Sub-agent companions, tmux jump, OpenCode via SQLite, process fallback, README with screenshots.

## Testing

- Parsers tested against small redacted fixture transcripts for each agent (written by hand, never copied from real sessions).
- `mapping.rs` is pure and unit-tested state by state.
- The lane view model and reducers are tested without a terminal.
- Snapshot tests only for stable pieces: the half-block packer and a single lane rendered at a fixed animation frame. No whole-screen snapshots that include clocks or counters.
- `runeway status --json` and `runeway check` are tested end to end against fixture directories.
- A frame-time benchmark at 160×48 cells; target under 4 ms per frame on this machine.
- Manual check in Ghostty at 2× DPR, and over SSH with `--lowres`.

## Decided by the builder (owner may override)

- Claude and Codex parsers in phase 1; OpenCode (SQLite) in phase 3; other agents run on agent-deck status alone.
- Sprites as in-source pixel maps rather than PNG files.
- Local git repo only until the owner asks to publish (then `cybercoretech/runeway`).
- Rust stable, with `rust-version` set to what the chosen crate versions require.

## Left to the builder

- Exact sprite designs, palette values and animation frame counts, within the anti-goals above.
- Scroll speed, physics constants and timings.
- Procedural level generation details.
