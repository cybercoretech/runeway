<div align="center">

# 🏃 Runeway

**A retro side-scrolling view of your running coding agents, in the terminal.**

🧙 one lane per agent · 🪙 tokens as coins · 🚪 a gate when an agent needs you · 🔒 read-only and offline

[![CI](https://github.com/cybercoretech/runeway/actions/workflows/ci.yml/badge.svg)](https://github.com/cybercoretech/runeway/actions/workflows/ci.yml)

</div>

Runeway watches your coding agents: the ones you run through [agent-deck](https://github.com/asheshgoplani/agent-deck) (Claude Code, Codex, OpenCode, Gemini and others), plus any Claude Code or Codex session that is busy on its own. It shows what each one is doing right now, how many tokens it has used, how full its context is, and which ones are waiting for your answer.

Every agent gets its own lane and an original pixel-art runner, in the spirit of the 8-bit side-scrollers of the 1980s. The whole cast, world and look are original.

## 🗺️ Status

| Phase | What | State |
|---|---|---|
| 1 | Data sources, `runeway status`, `runeway check`, live list view | ✅ done |
| 2 | The platformer: half-block pixel renderer, lanes, the original cast, HUD, agents outside agent-deck, `--demo`, CI | ✅ this branch |
| 3 | Jump to an agent's tmux pane, OpenCode transcripts, lane themes | 🔜 later |

## 📦 Install

Rust 1.88 or newer:

```bash
cargo install --path .
```

## 🎮 Use

```bash
runeway              # the world view (q, Esc or Ctrl-C to quit, ? for help)
runeway --demo       # four scripted agents, no real ones needed
runeway status       # one line per agent, then exit
runeway status --json
runeway check        # which sources were found; exits 1 if agent-deck can't be asked
```

| Key | Does |
|---|---|
| `↑` `↓` / `k` `j` | Select an agent |
| `l` | Switch between the world and the plain list |
| `?` | Show or hide the keys |
| `q` / `Esc` / `Ctrl-C` | Quit (the terminal is always restored) |

| Flag | Does |
|---|---|
| `--demo` | Play four scripted agents on a one-minute loop |
| `--lowres` | One pixel per cell, for SSH or slow terminals |
| `--fps N` | Frames per second while something moves (5–60, default 30; about 8 when only idle loops run) |
| `--no-discover` | Show only agent-deck's sessions |

## 🌃 The world

Each lane is 12 rows: a HUD row (name, tool, 🪙 tokens, a context meter `▰▰▰▱▱` that drains as the context fills, and what the agent is doing), then the level, drawn with half-block pixels (`▀`, two pixels per cell). The selected lane has a cyan edge; a lane waiting for you has a gold one. When there are more agents than fit, the rest become one line each and the footer says how many.

| Agent | Runner |
|---|---|
| 🧙 Claude Code | Hooded rune-runner with a cyan visor and a trailing cloak |
| ⚙️ Codex | Brass automaton with a chimney that puffs as it walks |
| 🛸 OpenCode | Hover-drone: a lens between two rotors |
| 🔮 Gemini | Twin-spirit: two linked orbs, violet and cyan |
| 🤖 Anyone else | Visored cyber-runner, tinted by the agent's name |

| The agent is… | The runner… |
|---|---|
| thinking | runs; the level scrolls past skyline, server racks or rune pillars |
| reading or searching | collects a glyph coin per file |
| editing | jumps and breaks a rune brick; a data gem pops out |
| running a command | passes turning gears and venting steam |
| failing a command | flashes red and stumbles back |
| passing a command | gets a shower of sparks |
| starting a helper | gains a small companion |
| waiting for you | stops at a rune gate, a gold glyph pulsing above |
| idle | sits down |
| finished | reaches the beacon tower |
| gone (error or stopped) | greys out: lost signal |

Example `runeway status`:

```text
  app   claude    running   editing lib.rs        12.4k tok   31%
! api   codex     waiting   waiting for you           0 tok     -
2 agents, 1 waiting
```

`!` marks an agent that is waiting for you, usually at a permission prompt.

## 🔌 Where the data comes from

| Source | Gives | Default location |
|---|---|---|
| 🃏 `agent-deck ls --json` | Which sessions exist, their status (`waiting` = needs you) and tmux session | `agent-deck` on your `PATH` |
| 🧙 Claude Code transcripts | Tool calls (reading, editing, commands), results, token use | `~/.claude/projects/` |
| 🤖 Codex rollouts | Commands and their results, token use and context window | `~/.codex/sessions/` |

🪙 Token counts cover what Runeway has read: the most recent part of each transcript when it starts (at most 256 KiB), then everything new. They are not a lifetime total, so they restart when Runeway restarts. The context % is always current.

Override any of them with `--agent-deck PATH`, `--claude-dir DIR` and `--codex-dir DIR`, or skip agent-deck with `--no-agent-deck`. Agents without a readable transcript still appear, using agent-deck's status alone.

🔭 **Agents outside agent-deck.** Any Claude Code or Codex transcript written to in the last 10 minutes gets a lane too, named after its project folder. It runs while the file grew in the last minute, sits after that, and leaves when the file has been quiet for 10 minutes. A session agent-deck already lists is not shown twice. Turn this off with `--no-discover`.

## 🔒 Privacy

- 👀 **Read-only.** Runeway never writes to, copies or deletes agent-deck data or transcripts.
- 📴 **Offline.** It makes no network connections.
- 🏷️ **Labels only.** It shows a short label such as `editing lib.rs` or `running cargo`. It never shows file contents, full commands, prompts or tool output.
- ⏪ **Only the recent past.** On start it reads at most the last 256 KiB of each transcript.

## 🧪 Develop

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The tests use hand-written fixtures under `tests/fixtures/`, never real transcripts or your home folder. The design and plans are in `docs/superpowers/`.

```bash
cargo run --example sprite_sheet               # every sprite at 4×, for a visual check
cargo run --release --example frame_bench      # frame time for 12 lanes at 160×48 (target < 4 ms)
RUNEWAY_UPDATE_SNAPSHOTS=1 cargo test lane     # rewrite the lane snapshot after a deliberate change
```

`tests/originality.rs` fails the build if a borrowed game name shows up in the code or this README.

---

<div align="center">
  <sub>👻 Code & Design by Darkstardevx · Cybercore Tech</sub>
</div>
