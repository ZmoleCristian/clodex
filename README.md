# Clodex

Codex CLI with Claude stuffed inside. An abomination. Sam and Dario would not hold hands, so we did it for them. 🦞🤝🦞

Clodex is a fork of [openai/codex](https://github.com/openai/codex) where the model behind the ChatGPT desktop app's Codex is Claude, talking to Anthropic's Messages API in-process. No proxy, no translation layer, no TypeScript. The desktop app, the voice mode and your ChatGPT login stay exactly as they are; only the backend binary changes.

Built in one day, in one sitting, by voice. Treat it accordingly.

## What it does

- `WireApi::Anthropic`: a new wire in `codex-rs/anthropic` that maps Codex's Responses-style prompt to Anthropic Messages (tools, thinking with signatures, prompt caching, images, native web search) and streams the result back as Codex events.
- Auth from Claude Code: reads `~/.claude/.credentials.json`, refreshes the OAuth token in place, retries once on 401. Your Claude subscription is the quota.
- Voice mode keeps working: the realtime call is created on your ChatGPT account, the backend that answers is Claude. A voice-mode addendum is appended to every realtime session so Claude knows how to behave as a voice backend.
- Subagents wake the parent when they finish.
- Threads created under another provider resume on the active one.

Twelve commits on top of `rust-v0.162.0-alpha.10`. `git log b496f984f4..HEAD` is the whole story.

## Heads up

- Tested on one machine: Arch Linux, ChatGPT desktop 26.930 (`chatgpt-bin`), Codex CLI 0.162.0-alpha.10 bundled. Nothing else.
- Needs an existing ChatGPT login in the desktop app (for the UI and voice) and an existing Claude Code login (for the model).
- Anthropic's OAuth is meant for Claude Code. Clodex presents itself as Claude Code. Whether that is inside the terms of your plan is your call.
- Unoptimized dev build, ~420 MB binary. No tests were run. No clippy. Expect rough edges: citations are dropped, `apply_patch` does not stream its diff, rate-limit headers are ignored.
- The desktop still runs the stock `codex-code-mode-host`; Clodex symlinks to it.

## Install

```bash
git clone https://github.com/ZmoleCristian/clodex
cd clodex/codex-rs
RUSTFLAGS="-C link-arg=-fuse-ld=mold" cargo build -p codex-cli --bin codex --profile dev-small
ln -s /usr/lib/chatgpt/resources/codex-code-mode-host target/dev-small/codex-code-mode-host
```

Cold build is about 5 minutes on 8 cores; incremental builds are 30 to 50 seconds.

A model catalog with the Claude models is needed. Put it at `~/.codex/catalog.clodex.json` (see `docs/clodex-catalog.example.json`).

Wrapper the desktop will launch instead of its bundled CLI:

```sh
#!/bin/sh
# ~/bin/clodex-codex
real=/path/to/clodex/codex-rs/target/dev-small/codex
case " $* " in
  *" app-server "*) exec "$real" "$@" \
    -c model_provider=anthropic \
    -c model=claude-opus-5-5 \
    -c model_catalog_json=$HOME/.codex/catalog.clodex.json ;;
  *) exec "$real" "$@" ;;
esac
```

Launcher:

```sh
#!/bin/sh
# ~/bin/clodex
pkill -f /usr/lib/chatgpt/ChatGPT 2>/dev/null
while pgrep -f /usr/lib/chatgpt/ChatGPT >/dev/null; do sleep 0.2; done
export CODEX_CLI_PATH=$HOME/bin/clodex-codex
exec /usr/lib/chatgpt/ChatGPT "$@"
```

The stock app, `/usr/lib/chatgpt` and `~/.codex` are untouched. Start the app from its menu entry and you get stock Codex; start it with `clodex` and you get Claude.

## Why

OpenCodex worked but it is a 480k-line TypeScript proxy between the app and the model. The seam in Codex is one match arm in `core/src/client.rs`; it was cheaper to put Anthropic there.

## Upstream

This fork tracks `openai/codex` tags. Rebase, resolve the handful of conflicts in `client.rs`, `realtime_conversation.rs` and `thread_processor.rs`, rebuild. The upstream README is in `docs/README.upstream.md`.
