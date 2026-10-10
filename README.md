# Clodex

Codex CLI with Claude stuffed inside. An abomination. Sam and Dario would not hold hands, so we did it for them. 🦞🤝🦞

Clodex is a fork of [openai/codex](https://github.com/openai/codex) where the model behind the ChatGPT desktop app's Codex is Claude, talking to Anthropic's Messages API in-process. No proxy, no translation layer, no TypeScript. The desktop app, the voice mode and your ChatGPT login stay as they are; only the backend binary changes.

Built in one sitting, by voice. Treat it accordingly.

## Docs

- [Install](docs/clodex/install.md): Arch-tested; ChatGPT desktop, Claude Code login, Rust + mold, build script, the `~/bin` wrappers.
- [Architecture](docs/clodex/architecture.md): the `codex-rs/anthropic` wire, provider wiring, built-in Claude catalog and combo slugs, exec policy, voice glue, auth, rate limits.
- [Voice mode](docs/clodex/voice-mode.md): what works on the desktop and what does not.
- [Changelog](docs/clodex/CHANGELOG.md): every commit since upstream `rust-v0.162.0-alpha.10`.
- [Upstream README](docs/README.upstream.md).

## Heads up

- Tested on one machine: Arch Linux, ChatGPT desktop 26.930 (`chatgpt-bin`), Codex CLI 0.162.0-alpha.10 bundled. Nothing else.
- Needs an existing ChatGPT login in the desktop app (UI and voice) and an existing Claude Code login (the model). Your Claude subscription is the quota.
- Anthropic's OAuth is meant for Claude Code. Clodex presents itself as Claude Code. Whether that is inside the terms of your plan is your call.
- Alpha. Unoptimized dev build, ~420 MB binary, no test suite run, no clippy. Expect rough edges.
- The desktop still runs the stock `codex-code-mode-host`; Clodex symlinks to it.

## Why

OpenCodex worked but it is a 480k-line TypeScript proxy between the app and the model. The seam in Codex is one match arm in `core/src/client.rs`; it was cheaper to put Anthropic there.

## Upstream

This fork tracks `openai/codex` tags. Rebase, resolve the handful of conflicts in `client.rs`, `realtime_conversation.rs` and `thread_processor.rs`, rebuild.

