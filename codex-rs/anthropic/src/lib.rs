//! Anthropic Messages API wire support for Codex.
//!
//! `request` turns a Codex prompt (ResponseItems + instructions + tools) into a Messages
//! request body; `sse` turns the Messages SSE stream back into `ResponseEvent`s.

mod auth;
mod request;
mod sse;

pub use auth::oauth_access_token;
pub use request::MessagesRequest;
pub use request::RequestInput;
pub use request::build_request;
pub use sse::spawn_response_stream;

use http::HeaderMap;
use http::HeaderValue;
use sha2::Digest;
use sha2::Sha256;

pub const MESSAGES_PATH: &str = "/v1/messages";
pub const API_VERSION: &str = "2023-06-01";
/// Betas required when authenticating with a Claude Pro/Max OAuth token.
pub const OAUTH_BETA: &str = "claude-code-20250219,oauth-2025-04-20";
/// Claude OAuth requires this exact text as the first system block.
pub const CLAUDE_CODE_SYSTEM_INSTRUCTION: &str =
    "You are a Claude agent, built on Anthropic's Claude Agent SDK.";
/// OAuth tokens reject arbitrary tool names; every Codex tool is prefixed on the wire.
pub const TOOL_PREFIX: &str = "custom_";
const SDK_VERSION: &str = "0.74.0";
/// Anthropic gates models by Claude Code version (`claude_code_version_too_old` below 2.1.280).
const USER_AGENT: &str = "claude-cli/2.1.300 (external, cli)";

/// Headers for an OAuth-authenticated streaming Messages request, including the Claude Code
/// client fingerprint the token was minted for.
pub fn oauth_headers(access_token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let mut put = |name: &'static str, value: String| {
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.insert(name, value);
        }
    };
    put("content-type", "application/json".into());
    put("accept", "text/event-stream".into());
    put("anthropic-version", API_VERSION.into());
    put("anthropic-beta", OAUTH_BETA.into());
    put("authorization", format!("Bearer {access_token}"));
    put("user-agent", USER_AGENT.into());
    put("x-app", "cli".into());
    put("x-stainless-retry-count", "0".into());
    put("x-stainless-runtime", "node".into());
    put("x-stainless-lang", "js".into());
    put("x-stainless-timeout", "600".into());
    put("x-stainless-arch", stainless_arch().into());
    put("x-stainless-os", stainless_os().into());
    put("x-stainless-package-version", SDK_VERSION.into());
    put("x-stainless-runtime-version", "22.12.0".into());
    put("x-claude-code-session-id", claude_code_session_id(access_token));
    put("x-client-request-id", uuid::Uuid::new_v4().to_string());
    headers
}

fn stainless_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn stainless_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// Deterministic UUIDv4-shaped session id derived from the token, mirroring Claude Code's
/// per-session id without persisting state.
fn claude_code_session_id(access_token: &str) -> String {
    let digest = Sha256::digest(format!("claude-code-session:{access_token}").as_bytes());
    let h: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let variant = (u8::from_str_radix(&h[16..17], 16).unwrap_or(0) & 0x3) | 0x8;
    format!(
        "{}-{}-4{}-{variant:x}{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        &h[17..20],
        &h[20..32]
    )
}
