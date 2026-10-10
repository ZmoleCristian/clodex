//! Clodex exec policy: hard timeouts and one-liner rejection for shell commands.
//!
//! Always on. Long work goes through `setsid nohup ... &`; everything else is
//! killed when its foreground wait expires.

use std::time::Duration;

pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 15_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 60_000;
const MAX_COMMAND_CHARS: usize = 300;

pub(crate) fn clamp_timeout_ms(requested: u64) -> u64 {
    requested.min(MAX_TIMEOUT_MS)
}

pub(crate) fn killed_marker(wait: Duration) -> String {
    format!("\n[clodex: killed after {}s timeout]", wait.as_secs())
}

pub(crate) fn rejection_message(reason: &str) -> String {
    format!(
        "clodex: {reason}. Split into separate calls, or run detached with setsid nohup ... > /tmp/x.log 2>&1 &"
    )
}

/// Text outside single/double quotes, quoted spans replaced by a space.
fn unquoted(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in cmd.chars() {
        match quote {
            Some(q) => {
                if c == q && !escaped {
                    quote = None;
                    out.push(' ');
                }
                escaped = q == '"' && c == '\\' && !escaped;
            }
            None => {
                if escaped {
                    escaped = false;
                    out.push(' ');
                } else if c == '\\' {
                    escaped = true;
                } else if c == '\'' || c == '"' {
                    quote = Some(c);
                } else {
                    out.push(c);
                }
            }
        }
    }
    out
}

pub(crate) fn reject_reason(cmd: &str) -> Option<String> {
    let trimmed = cmd.trim_start();
    if trimmed.starts_with("setsid nohup") || trimmed.starts_with("apply_patch") {
        return None;
    }
    let len = cmd.chars().count();
    if len > MAX_COMMAND_CHARS {
        return Some(format!("command is {len} chars, max {MAX_COMMAND_CHARS}"));
    }
    let text = unquoted(cmd);
    if text.contains("&&") {
        return Some("\"&&\" chain".into());
    }
    if text.contains(';') {
        return Some("\";\" separator".into());
    }
    if text.contains("<<") {
        return Some("heredoc \"<<\"".into());
    }
    if text.contains("| while") || text.contains("|while") {
        return Some("\"| while\" loop".into());
    }
    if text.contains("tail -f") || text.contains("tail -F") || text.contains("tail --follow") {
        return Some("\"tail -f\"".into());
    }
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '|' | '&' | '{' | '}' | '`'))
        .filter(|w| !w.is_empty())
        .collect();
    let has = |w: &str| words.iter().any(|x| *x == w);
    if has("do") && has("for") {
        return Some("\"for ... do\" loop".into());
    }
    if has("do") && has("while") {
        return Some("\"while ... do\" loop".into());
    }
    for (word, reason) in [
        ("watch", "\"watch\""),
        ("sleep", "\"sleep\""),
        ("read", "\"read\""),
    ] {
        if has(word) {
            return Some(format!("{reason} blocks the turn"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::reject_reason;

    #[test]
    fn allows_plain_and_detached() {
        assert_eq!(reject_reason("ls -la"), None);
        assert_eq!(reject_reason("rg -n 'for x; do' src"), None);
        assert_eq!(reject_reason("echo \"a && b; sleep 5\""), None);
        assert_eq!(reject_reason("find . -name x -exec rm {} \\;"), None);
        assert_eq!(
            reject_reason("setsid nohup bash -c 'sleep 30 && echo done' > /tmp/x.log 2>&1 &"),
            None
        );
    }

    #[test]
    fn rejects_blockers() {
        assert!(reject_reason("sleep 30; echo done").is_some());
        assert!(reject_reason("a && b").is_some());
        assert!(reject_reason("cat x | while read l do echo $l done").is_some());
        assert!(reject_reason("for f in *.rs do cat $f done").is_some());
        assert!(reject_reason("tail -f /tmp/x.log").is_some());
        assert!(reject_reason("watch ls").is_some());
        assert!(reject_reason("sleep 20").is_some());
        assert!(reject_reason("cat <<EOF").is_some());
        assert!(reject_reason(&"x".repeat(301)).is_some());
    }
}
