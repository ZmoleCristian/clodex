# Voice mode

The ChatGPT desktop app creates the realtime call on your ChatGPT account (realtime v1, existing call). The model that does the work behind the voice is Claude. Nothing in the desktop changes.

## What works

- Existing-call attach: the desktop creates the call, the app-server attaches to it with the ChatGPT bearer and account id (`prepare_realtime_start` in `codex-rs/core/src/realtime_conversation.rs`).
- Stop by voice: a handoff whose transcript is a stop word (stop, wait, hold on, cancel, abort, and the Romanian stai, oprește, așteaptă, lasă, anulează) interrupts the running turn before routing (`clodex_is_stop_intent`). The next request starts a new task.
- `[STATUS]` / `[COMPLETE]`: the frontend speaks tagged backend messages. Untagged text Claude emits between tool calls is tagged `[STATUS]` by the harness so it is spoken instead of dropped.
- Inline split: when Claude puts a `[STATUS]` and a `::codex-realtime-inline{}` item in one message, `handoff_out` splits them into two items, in order.
- No startup context item on existing calls: anything injected into the desktop call gets read aloud, so nothing is sent. The backend has the thread and opens its first delegation with a short status (rule in `clodex_voice_addendum.md`).
- Backend rules: `clodex_voice_addendum.md` is appended to every realtime start instruction (worker subagents for long tasks, detached commands, status cadence, inline item last). A combo slug adds `clodex_dual_addendum.md` (one persistent Builder agent on the build model).

## Known limits

- The desktop ignores `session.update`. Everything Clodex sends there is dead on the desktop: `clodex_frontend_addendum.md` (appended in `build_realtime_session_config`) and the v1 `interrupt_background_agent` tool (`codex-api/src/endpoint/realtime_websocket/methods_v1.rs`). The frontend prompt is whatever the desktop ships. Stop works only through the handoff path above.
- No silent context channel: every conversation item reaches the voice model as something to respond to. That is why the startup context was dropped from existing calls.
- No wake on subagent completion. The parent-wake commit was reverted (`789038de26`); a worker finishing does not produce a spoken "done" until the next delegation reads its state.
- Standalone websocket sessions (non-desktop) still get the startup context and the frontend addendum; that path is built but untested here.

