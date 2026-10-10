# Install (Arch Linux)

Tested on one machine: Arch, ChatGPT desktop 26.930 (`chatgpt-bin`), bundled Codex CLI 0.162.0-alpha.10.

## Prerequisites

- ChatGPT desktop app at `/usr/lib/chatgpt`, logged in (the UI and the realtime call use that login).
- Claude Code logged in: `~/.claude/.credentials.json` must contain `claudeAiOauth`. Clodex refreshes the token in that file.
- Rust toolchain (the workspace `rust-toolchain.toml` applies) and `mold`.

## Build

```sh
git clone https://github.com/ZmoleCristian/clodex
cd clodex/codex-rs
setsid nohup ./clodex-build.sh clodex > /dev/null 2>&1 &
```

`clodex-build.sh <name>` runs `cargo build -p codex-cli --bin codex --profile dev-small` with mold, logs to `/tmp/<name>.log` and writes `exit=<code> secs=<n>` to `/tmp/<name>.done`. Cold build ~5 min on 8 cores; incremental 30 to 50 s (core only) or ~3 min (codex-api/anthropic). Output: `codex-rs/target/dev-small/codex`, ~420 MB.

The desktop also runs the stock `codex-code-mode-host`; symlink it next to the binary:

```sh
ln -s /usr/lib/chatgpt/resources/codex-code-mode-host target/dev-small/codex-code-mode-host
```

## Wrappers

Two shell scripts in `~/bin`. The desktop launches whatever `CODEX_CLI_PATH` points to.

`~/bin/clodex-codex` is the drop-in for the desktop's Codex CLI. It runs the built binary with the same `CODEX_HOME` and `config.toml`; only `app-server` invocations get `-c model_provider=anthropic -c model=claude-opus-5-5` appended (last `-c` wins). Every other subcommand runs the binary unchanged. Set `real=` to your checkout's `codex-rs/target/dev-small/codex`.

`~/bin/clodex` is the launcher: kills the running ChatGPT desktop (`pkill -f /usr/lib/chatgpt/ChatGPT`, waits up to 10 s, then `pkill -9`), unsets `CODEX_ANTHROPIC_AUTH_FILE`, exports `CODEX_CLI_PATH=$HOME/bin/clodex-codex` and execs `/usr/lib/chatgpt/ChatGPT` with its arguments.

The stock app, `/usr/lib/chatgpt` and `~/.codex` stay untouched. Menu entry gives stock Codex; `clodex` gives Claude.

## Model

Pick any Claude slug from the in-app model picker; the catalog is built in (`codex-rs/models-manager/src/claude_catalog.rs`). Change the default by editing the `-c model=` in `clodex-codex`. Combo slugs like `claude-sonnet-5-5+claude-opus-5-5` run chat on the first model and spawned agents on the second.

## Env

- `CODEX_ANTHROPIC_AUTH_FILE`: alternative credentials file (same JSON shape as Claude Code's).

