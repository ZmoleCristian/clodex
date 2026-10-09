//! Claude Code OAuth credentials: read from `~/.claude/.credentials.json`, refreshed in place.

use codex_http_client::HttpTransport;
use codex_http_client::Request;
use codex_http_client::TransportError;
use http::Method;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const TOKEN_URLS: [&str; 2] = [
    "https://platform.claude.com/v1/oauth/token",
    "https://console.anthropic.com/v1/oauth/token",
];
const REFRESH_MARGIN_MS: u64 = 5 * 60 * 1000;

static CACHE: Mutex<Option<Credentials>> = Mutex::new(None);

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credentials {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
}

impl Credentials {
    fn usable(&self, rejected: Option<&str>) -> bool {
        self.expires_at > now_ms() + REFRESH_MARGIN_MS && rejected != Some(self.access_token.as_str())
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

/// Access token for the Messages API, refreshed when it expires within five minutes.
/// Pass the token the API just rejected with 401 as `rejected` to force a refresh.
pub async fn oauth_access_token(
    transport: &impl HttpTransport,
    rejected: Option<&str>,
) -> io::Result<String> {
    if let Some(creds) = cache().clone().filter(|creds| creds.usable(rejected)) {
        return Ok(creds.access_token);
    }
    let path = credentials_path()?;
    let mut creds = read_credentials(&path)?;
    if !creds.usable(rejected) {
        creds = refresh(transport, &creds.refresh_token).await?;
        write_credentials(&path, &creds)?;
    }
    *cache() = Some(creds.clone());
    Ok(creds.access_token)
}

fn cache() -> MutexGuard<'static, Option<Credentials>> {
    CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `$CODEX_ANTHROPIC_AUTH_FILE`, else Claude Code's `~/.claude/.credentials.json`.
fn credentials_path() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("CODEX_ANTHROPIC_AUTH_FILE").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".claude/.credentials.json"))
        .ok_or_else(|| io::Error::other("HOME not set"))
}

fn read_credentials(path: &Path) -> io::Result<Credentials> {
    let root: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    serde_json::from_value(root["claudeAiOauth"].clone())
        .map_err(|err| io::Error::other(format!("{}: claudeAiOauth: {err}", path.display())))
}

/// Updates the token fields under `claudeAiOauth` and replaces the file atomically (0600).
fn write_credentials(path: &Path, creds: &Credentials) -> io::Result<()> {
    let mut root: Value = std::fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_else(|| json!({}));
    let oauth = &mut root["claudeAiOauth"];
    if !oauth.is_object() {
        *oauth = json!({});
    }
    oauth["accessToken"] = json!(creds.access_token);
    oauth["refreshToken"] = json!(creds.refresh_token);
    oauth["expiresAt"] = json!(creds.expires_at);

    let tmp = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(&tmp)?, root.to_string().as_bytes())?;
    std::fs::rename(&tmp, path)
}

async fn refresh(transport: &impl HttpTransport, refresh_token: &str) -> io::Result<Credentials> {
    let body = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLIENT_ID,
    });
    let mut last_err = None;
    for url in TOKEN_URLS {
        let request = Request::new(Method::POST, url.to_string()).with_json(&body);
        match transport.execute(request).await {
            Ok(response) => {
                let token: TokenResponse = serde_json::from_slice(&response.body)?;
                return Ok(Credentials {
                    access_token: token.access_token,
                    refresh_token: token
                        .refresh_token
                        .unwrap_or_else(|| refresh_token.to_string()),
                    expires_at: now_ms() + token.expires_in * 1000,
                });
            }
            // The token endpoint answered; a second host would only repeat the rejection.
            Err(err @ TransportError::Http { .. }) => {
                return Err(io::Error::other(format!("anthropic token refresh: {err}")));
            }
            Err(err) => last_err = Some(err),
        }
    }
    Err(io::Error::other(format!(
        "anthropic token refresh unreachable: {}",
        last_err.map(|err| err.to_string()).unwrap_or_default()
    )))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
