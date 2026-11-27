<div align="center">

# 🏃 Runeway

**A retro side-scrolling view of your running coding agents, in the terminal.**

🧙 one lane per agent · 🪙 tokens as coins · 🚪 a gate when an agent needs you · 🔒 read-only and offline

</div>

Runeway watches the coding agents you run through [agent-deck](https://github.com/asheshgoplani/agent-deck): Claude Code, Codex, OpenCode, Gemini and others. It shows what each one is doing right now, how many tokens it has used, how full its context is, and which ones are waiting for your answer.

The finished app gives every agent its own lane and an original pixel-art runner, in the spirit of the 8-bit platformers of the 1980s. The whole cast, world and look are original.

## 🗺️ Status

| Phase | What | State |
|---|---|---|
| 1 | Data sources, `runeway status`, `runeway check`, live list view | ✅ this branch |
| 2 | The platformer: half-block pixel renderer, lanes, the original cast, HUD | 🔜 next |
| 3 | Every agent state animated, jump to an agent's tmux pane, OpenCode support | 🔜 later |

## 📦 Install

Rust 1.88 or newer:

```bash
cargo install --path .
```

## 🎮 Use

```bash
runeway              # live view (q, Esc or Ctrl-C to quit, ? for help)
runeway status       # one line per agent, then exit
runeway status --json
runeway check        # which sources were found; exits 1 if agent-deck can't be asked
```

| Key | Does |
|---|---|
| `↑` `↓` / `k` `j` | Select an agent |
| `?` | Show or hide the keys |
| `q` / `Esc` / `Ctrl-C` | Quit (the terminal is always restored) |

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

Override any of them with `--agent-deck PATH`, `--claude-dir DIR` and `--codex-dir DIR`, or skip agent-deck with `--no-agent-deck`. Agents without a readable transcript still appear, using agent-deck's status alone.

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

---

<div align="center">
  <sub>👻 Code & Design by Darkstardevx · Cybercore Tech</sub>
</div>
