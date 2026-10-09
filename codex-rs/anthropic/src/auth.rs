use serde_json::Value;
use std::io;

const DEFAULT_STATE_FILE: &str =
    "/home/tragdate/codumeu/struto-camila/.tmp/struto-camila/state/auth.json";

/// Reads the active Anthropic OAuth access token from the OpenCodex state file.
/// Override the path with `CODEX_ANTHROPIC_AUTH_FILE`.
pub fn oauth_access_token() -> io::Result<String> {
    let path = std::env::var("CODEX_ANTHROPIC_AUTH_FILE")
        .ok()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_STATE_FILE.to_string());
    let raw = std::fs::read(&path)?;
    let state: Value = serde_json::from_slice(&raw)?;
    let anthropic = &state["anthropic"];
    let active = anthropic["activeAccountId"].as_str();
    let accounts = anthropic["accounts"].as_array().cloned().unwrap_or_default();
    let account = accounts
        .iter()
        .find(|account| active.is_some_and(|id| account["id"].as_str() == Some(id)))
        .or_else(|| accounts.first())
        .ok_or_else(|| io::Error::other(format!("no anthropic account in {path}")))?;
    account["credential"]["access"]
        .as_str()
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| io::Error::other(format!("anthropic access token missing in {path}")))
}
