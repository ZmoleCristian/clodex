# Architecture

Clodex is upstream Codex with one extra wire. The ChatGPT desktop app launches `codex app-server`; the wrapper appends `-c model_provider=anthropic -c model=claude-opus-5-5`; every chat turn then goes to `api.anthropic.com/v1/messages` in-process. No proxy, no second process.

## The seam

`codex-rs/core/src/client.rs`, `ModelClientSession::stream`: the match on `WireApi` gained an `Anthropic` arm that calls `stream_anthropic`. It builds the request with `codex_anthropic::build_request`, fetches the OAuth token, POSTs, and feeds the SSE response through `codex_anthropic::spawn_response_stream` into the same `map_response_stream` the Responses wire uses. Core sees ordinary `ResponseEvent`s.

## Provider wiring

- `codex-rs/model-provider-info/src/lib.rs`: `WireApi::Anthropic` (config value `"anthropic"`), provider id `anthropic`, base URL `https://api.anthropic.com`, registered in `built_in_model_providers`. `requires_openai_auth` is `true` on purpose: the desktop derives its login state and the voice button from `account/read`; Claude requests carry their own token anyway.
- `codex-rs/model-provider/src/provider.rs`: on the Anthropic wire the models manager is a `StaticModelsManager` fed by the built-in Claude catalog; `model_catalog_json` is ignored there.
- `codex-rs/app-server/src/request_processors/thread_processor.rs`: a resumed thread whose persisted `model_provider` differs from the active one keeps the active provider and model, so threads born under stock Codex resume on Claude and vice versa.

## codex-rs/anthropic

| file | job |
| --- | --- |
| `lib.rs` | constants (`/v1/messages`, API version, OAuth betas, Claude Code user agent) and `oauth_headers`, which fingerprints the request as Claude Code; tool names get the `custom_` prefix because OAuth tokens reject arbitrary names |
| `request.rs` | `build_request`: Codex prompt (instructions, `ResponseItem`s, tools) to a Messages body. System block opens with the mandatory Claude Code sentence; `cache_control` on system, tools and last message; thinking blocks replayed only with a native Anthropic signature (foreign or missing signatures become `[thinking]` text); images as inline or URL blocks; `web_search_20250305` server tool when search is enabled; `output_schema` as a forced tool |
| `sse.rs` | Messages SSE to `ResponseEvent`: text deltas, thinking with signatures, `tool_use` as function calls (freeform tools such as `apply_patch` as `CustomToolCall`), namespaces restored from the prefixed name, usage, and `anthropic-ratelimit-*` headers as `ResponseEvent::RateLimits` |
| `auth.rs` | Claude Code credentials, below |
| `rate_limits.rs` | header parsing, below |

Thinking: `ThinkingMode` on `ModelInfo` (`codex-rs/protocol/src/openai_models.rs`). `Adaptive` sends `thinking.type: adaptive` plus `output_config.effort`; `Budget` sends `thinking.type: enabled` with `budget_tokens` derived from the reasoning effort.

## Built-in Claude catalog

`codex-rs/models-manager/src/claude_catalog.rs` lists the slugs the picker shows (Opus, Sonnet, Haiku, Fable 5.x and 4.x, each with a thinking mode and auto-compact limit): 1M context window, unified exec shell, freeform `apply_patch`, code mode only, multi-agent v2, `claude_prompt.md` as instructions template.

Combo slugs: `chat+build`, for example `claude-sonnet-5-5+claude-opus-5-5` ("Sonnet / Opus"). `chat_model()` strips the slug to the chat half before it hits the wire; `codex-rs/core/src/agent/child_config.rs` makes every spawned agent without an explicit model use the build half. In voice mode a combo slug also appends the dual addendum (one persistent "Builder" agent).

## Exec policy

`codex-rs/core/src/clodex_exec_policy.rs`, always on: shell commands default to a 15 s timeout, capped at 60 s, killed with a `[clodex: killed after Ns timeout]` marker. Commands over 300 chars, `&&` or `;` chains, loops and heredocs are rejected with a hint to split or run detached.

## Voice mode glue

`codex-rs/core/src/realtime_conversation.rs`:

- `prepare_realtime_start`: when the chat provider is Anthropic the realtime call is created on the OpenAI provider with the ChatGPT login (bearer plus `chatgpt-account-id`); the WebRTC call goes through `chatgpt.com/backend-api/codex`.
- `build_realtime_session_config`: always appends `clodex_frontend_addendum.md` to the frontend instructions; existing (desktop) calls get no startup context item.
- `handoff_out`: splits a message carrying both a `[STATUS]` and a `::codex-realtime-inline{}` directive into two items; tags untagged backend text as `[STATUS]` so the frontend speaks it.
- `handle_start_inner`: `InterruptRequested` events and handoffs whose transcript is a stop word (`clodex_is_stop_intent`, en/ro) interrupt the running turn.

`codex-rs/core/src/context/world_state/realtime.rs`: backend start instructions are the desktop's (or upstream default) text plus `clodex_voice_addendum.md`, plus `clodex_dual_addendum.md` under a combo slug.

`codex-rs/core/src/realtime_context.rs`: the startup context thread section lists the last three user requests, last tagged status, files changed by `apply_patch` in the last turn, open subagents. Only reaches fresh (non-desktop) sessions.

Templates: `codex-rs/prompts/templates/realtime/clodex_{voice,frontend,dual}_addendum.md`, exported from `codex-rs/prompts/src/realtime.rs`.

## Auth

`codex-rs/anthropic/src/auth.rs` reads `claudeAiOauth` from `~/.claude/.credentials.json` (override: `CODEX_ANTHROPIC_AUTH_FILE`). The token is cached in-process; when it expires within five minutes, or the API answered 401, it is refreshed against `platform.claude.com` (fallback `console.anthropic.com`) with Claude Code's client id and written back atomically, mode 0600. `stream_anthropic` retries once after a 401.

## Rate limits

`codex-rs/anthropic/src/rate_limits.rs` maps the OAuth family `anthropic-ratelimit-unified-{5h,7d}-{utilization,reset}` (primary 5h, secondary 7d) or the API-key family `anthropic-ratelimit-{tokens,requests}-*` to a `RateLimitSnapshot` with `limit_id = "anthropic"`, so the desktop's usage display shows the Claude quota.

