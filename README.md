# aimon

Tiny TUI that shows what your terminal coding agents (Claude Code, Codex, Gemini CLI, Aider,
OpenCode, cursor-agent, Amp, Goose, Crush, Qwen, Copilot, Droid — in tmux or not) spawned, warns before
macOS runs out of memory, and kills the leftovers before they freeze the machine.

The usual culprit: a session dies or is interrupted mid `vitest`/`jest`/`tsserver`/MCP run, the
workers get re-parented to launchd (PPID 1), vanish from the tmux tree, and keep eating RAM.
Activity Monitor blames iTerm2.

~600 KB binary, ~6 MB RSS, ~0.1% CPU idle. macOS only.

## Install

Universal macOS binary from [releases](https://github.com/CarlosDanielDev/aimon/releases/latest), or:

```sh
cargo install --git https://github.com/CarlosDanielDev/aimon
```

## Use

```sh
aimon                 # TUI
aimon --daemon        # no UI: auto-kill + alerts only (tmux pane or launchd agent)
aimon --no-auto       # never kill on its own
```

| flag | default | |
|---|---|---|
| `--rss-limit-mb N` | 500 | a dev child of an agent above this is a panic target |
| `--orphan-grace-s N` | 60 | an orphaned tool worker is killed after N seconds |
| `--tick-ms N` | 2000 | sampling interval (×2.5 when the terminal loses focus) |

Keys: `↑↓`/`jk` move · `x` kill selected · `X` kill the group's dev processes · `p` panic ·
`a` toggle auto · `t` theme (neon/matrix) · `q` quit. `NO_COLOR` is honored.

## Rules

- **Protected, never touched:** the agents themselves, `tmux`, shells, `nvim`/`vim`, iTerm2, apps in
  `/Applications` and `/System`.
- **Orphan tool worker** (`vitest|jest|esbuild|tsserver|playwright|mcp|pytest`, parent is
  launchd): killed automatically after the grace period, and pre-marked in panic.
- **Orphan runtime** (`node`, `python`, `bun`, …): shown in ÓRFÃOS but never pre-marked or
  auto-killed, since it may be a daemon you started on purpose. Kill it with `x`.
- **Heavy child of an agent** (a dev process above `--rss-limit-mb`): pre-marked in panic.
- **Critical memory pressure** (`kern.memorystatus_vm_pressure_level`): auto mode kills the
  heaviest panic target every 10 s until pressure drops.

Kills are SIGTERM, then SIGKILL 2 s later. Every kill sends a macOS notification and appends a
line to `~/.local/state/aimon/kills.log`.

## Develop

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

CI runs that gate on every push and PR. To release, bump `version` in `Cargo.toml` and push to
`main`: CI tags `vX.Y.Z` and publishes a universal (arm64 + x86_64) binary with its sha256.

## License

[GPL-3.0-or-later](LICENSE)
