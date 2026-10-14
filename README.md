# aimon

<p align="left">
  <a href="https://github.com/CarlosDanielDev/aimon/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/CarlosDanielDev/aimon/ci.yml?branch=main&label=ci&style=flat-square" alt="ci" height="20"></a>
  <a href="https://github.com/CarlosDanielDev/aimon/releases/latest"><img src="https://img.shields.io/github/v/release/CarlosDanielDev/aimon?style=flat-square" alt="release" height="20"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/CarlosDanielDev/aimon?style=flat-square" alt="license" height="20"></a>
  <img src="https://img.shields.io/badge/platform-macos%20%7C%20linux%20%7C%20windows-2ea4a4?style=flat-square" alt="platform" height="20">
  <img src="https://img.shields.io/badge/rust-2024%20edition-dea584?style=flat-square&logo=rust&logoColor=white" alt="rust" height="20">
</p>

<p align="center"><img src="assets/mascot.png" width="260" alt="aimon mascot: a pixel guardian creature sweeps orphan processes away"></p>

Tiny TUI that shows what your terminal coding agents (Claude Code, Codex, Gemini CLI, Aider,
OpenCode, cursor-agent, Amp, Goose, Crush, Qwen, Copilot, Droid — in tmux or not) spawned, warns before
your machine runs out of memory, and kills the leftovers before they freeze it.

The usual culprit: a session dies or is interrupted mid `vitest`/`jest`/`tsserver`/MCP run, the
workers get re-parented to launchd (PPID 1), vanish from the tmux tree, and keep eating RAM.
Activity Monitor blames iTerm2.

~600 KB binary, ~6 MB RSS, ~0.1% CPU idle. macOS, Linux and Windows.

## Install

Statically-linked binaries from [releases](https://github.com/CarlosDanielDev/aimon/releases/latest):
`aimon-vX.Y.Z-macos-universal.tar.gz`, `aimon-vX.Y.Z-linux-x86_64.tar.gz`,
`aimon-vX.Y.Z-linux-aarch64.tar.gz`, `aimon-vX.Y.Z-windows-x86_64.zip` — or:

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
`a` toggle auto · `t` theme (neon/matrix) · `q` quit. `x`, `X` and `p` all open the same
confirm popup: rows arrive pre-marked, `space` toggles, `enter` fires, `esc` cancels. The
cursor follows its process when the list re-sorts, so it never kills the row that moved
under it. `NO_COLOR` is honored.

## Rules

- **Protected, never touched:** the agents themselves, `tmux`, shells, `nvim`/`vim`, iTerm2, apps in
  `/Applications` and `/System`.
- **Orphan tool worker** (`vitest|jest|esbuild|tsserver|playwright|mcp|pytest`, parent is
  launchd): killed automatically after the grace period, and pre-marked in panic.
- **Orphan runtime** (`node`, `python`, `bun`, …): shown in ORPHANS but never pre-marked or
  auto-killed, since it may be a daemon you started on purpose. Kill it with `x`.
- **Heavy child of an agent** (a dev process above `--rss-limit-mb`): pre-marked in panic.
- **Critical memory pressure:** auto mode kills the heaviest panic target every 10 s until
  pressure drops. macOS reads `kern.memorystatus_vm_pressure_level`, Linux reads PSI
  (`/proc/pressure/memory`); Windows has no kernel pressure signal, so the critical rule is off
  there — orphan reaping, panic and manual kills all still work (kills are immediate Terminate).

Kills are SIGTERM, then SIGKILL 2 s later (Windows: TerminateProcess). Every kill sends a native
notification and appends a line to `~/.local/state/aimon/kills.log`.

## Develop

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

CI runs that gate on every push and PR. To release, bump `version` in `Cargo.toml` and push to
`main`: CI tags `vX.Y.Z` and publishes macOS (universal), Linux (x86_64 + aarch64, musl) and
Windows (x86_64) binaries with their sha256s.

## License

[GPL-3.0-or-later](LICENSE)

<p align="center"><a href="https://viberank.dev/apps/aimon" target="_blank" rel="noopener noreferrer"><img src="https://viberank.dev/badge?app=aimon&theme=dark" alt="aimon on VibeRank" /></a></p>
