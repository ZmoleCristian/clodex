//! Anthropic `anthropic-ratelimit-*` response headers -> `RateLimitSnapshot`.
//!
//! Two header families exist: the API-key family
//! (`anthropic-ratelimit-{tokens,requests}-{limit,remaining,reset}`, RFC 3339 reset)
//! and the OAuth subscription family
//! (`anthropic-ratelimit-unified-{5h,7d}-{utilization,reset}`, utilization 0..1,
//! reset unix seconds). Primary window = tokens / 5h, secondary = requests / 7d.

use chrono::DateTime;
use codex_protocol::protocol::RateLimitSnapshot;
use codex_protocol::protocol::RateLimitWindow;
use http::HeaderMap;

pub fn parse_rate_limits(headers: &HeaderMap) -> Option<RateLimitSnapshot> {
    let primary = unified_window(headers, "5h", 5 * 60)
        .or_else(|| quota_window(headers, "tokens"))
        .or_else(|| quota_window(headers, "input-tokens"));
    let secondary = unified_window(headers, "7d", 7 * 24 * 60)
        .or_else(|| quota_window(headers, "requests"))
        .or_else(|| quota_window(headers, "output-tokens"));
    if primary.is_none() && secondary.is_none() {
        return None;
    }
    Some(RateLimitSnapshot {
        limit_id: Some("anthropic".to_string()),
        limit_name: Some("Anthropic".to_string()),
        normal_model_slug: None,
        primary,
        secondary,
        credits: None,
        individual_limit: None,
        spend_control_reached: None,
        plan_type: None,
        rate_limit_reached_type: None,
    })
}

fn unified_window(headers: &HeaderMap, window: &str, minutes: i64) -> Option<RateLimitWindow> {
    let utilization = header_f64(
        headers,
        &format!("anthropic-ratelimit-unified-{window}-utilization"),
    )?;
    let resets_at = header_str(
        headers,
        &format!("anthropic-ratelimit-unified-{window}-reset"),
    )
    .and_then(parse_reset);
    Some(RateLimitWindow {
        used_percent: (utilization * 100.0).clamp(0.0, 100.0),
        window_minutes: Some(minutes),
        resets_at,
    })
}

fn quota_window(headers: &HeaderMap, quota: &str) -> Option<RateLimitWindow> {
    let limit = header_f64(headers, &format!("anthropic-ratelimit-{quota}-limit"))?;
    let remaining = header_f64(headers, &format!("anthropic-ratelimit-{quota}-remaining"))?;
    if limit <= 0.0 {
        return None;
    }
    let resets_at = header_str(headers, &format!("anthropic-ratelimit-{quota}-reset"))
        .and_then(parse_reset);
    Some(RateLimitWindow {
        used_percent: ((limit - remaining) / limit * 100.0).clamp(0.0, 100.0),
        window_minutes: Some(1),
        resets_at,
    })
}

fn parse_reset(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if let Ok(secs) = raw.parse::<i64>() {
        return Some(secs);
    }
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

fn header_f64(headers: &HeaderMap, name: &str) -> Option<f64> {
    header_str(headers, name)?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}
