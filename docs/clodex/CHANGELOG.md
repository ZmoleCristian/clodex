# Changelog

Base: upstream `rust-v0.162.0-alpha.10` (`b496f984f4`). Everything below is on top of it; `git log rust-v0.162.0-alpha.10..master` is the full list.

## 2026-10-10

- Voice addendum: no sleep on the main thread; inline items are separate messages (`9a025620`).
- Voice: drop the startup context item on existing calls; harness splits `[STATUS]`/inline messages (`4b6ec8f1`, `fc4d83c6`, `91a082ae`).
- Dual-model combo slugs: chat model on the main thread, build model for spawned agents, Builder addendum in voice start (`33e2e46f`).
- anthropic: freeform tool calls stream as `CustomToolCall`; namespaces replayed; unsigned reasoning and agent messages handled (`91ede243`).
- anthropic: `anthropic-ratelimit-*` headers surface as `ResponseEvent::RateLimits` (`f6c55f4d`).
- Revert "Wake the parent thread when a subagent finishes" (`789038de`).
- Voice: stop intent wording for the frontend, wider stop keyword list; a handoff carrying a spoken stop interrupts the running turn (`d70d2fb9`, `8b79d679`).
- Voice: interrupt tool on v1 sessions; untagged backend messages auto-tagged `[STATUS]` (`fceb312d`).
- Voice: frontend tool addendum always appended; detached build script `codex-rs/clodex-build.sh` (`865e60ac`).
- anthropic: skip OpenAI-signed thinking blocks on replay; drop the external catalog example (`c30ec880`).
- Built-in Claude model catalog on the Anthropic wire (`cd7991f6`).
- Exec policy: hard timeouts (15 s default, 60 s max) and rejection of one-liner shell commands (`7e5526a8`).
- Voice: `interrupt_background_agent` tool, startup context always on, status cadence in the addendum (`6c27ba75`).
- anthropic: web search citations kept as inline markdown links (`88e7ff5e`).

## 2026-10-09

- Remove the temporary realtime debug log (`068ae2c8`).
- Clodex README; upstream README moved to `docs/README.upstream.md` (`34b35ad2`).
- Drop upstream CODEOWNERS, dependabot, issue templates and GitHub workflows (`e7e2a76b`, `2413396b`).
- Realtime call created on the ChatGPT backend when the login is ChatGPT; ChatGPT bearer and account id on standalone realtime websockets; WebRTC call on the OpenAI provider when chat runs on Anthropic (`77f6c804`, `4db71970`, `55571d1e`).
- anthropic: skip replaying thinking blocks with foreign signature schemes (`3e560739`).
- Realtime voice routed to OpenAI when the chat provider is Anthropic; threads resume on the active provider (`83c1f26b`).
- anthropic: read Claude Code credentials, refresh OAuth token, retry once on 401 (`01e3e306`).
- anthropic: native web search via the `web_search_20250305` server tool (`88493c1b`).
- Wake the parent thread when a subagent finishes (`f023393d`, reverted 2026-10-10).
- Clodex voice addendum appended to every realtime start instruction (`88e48a65`).
- Call-less `FunctionCallOutput` items delivered as user text on the Anthropic wire (`97452ce9`).
- Anthropic provider reports OpenAI auth so the desktop keeps its login state (`d42ab4d2`).
- In-process Anthropic Messages API wire, crate `codex-anthropic` (`818b1755`).

