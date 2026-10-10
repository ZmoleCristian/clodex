use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::RateLimitSnapshot;
use codex_protocol::protocol::RateLimitWindow;
use codex_protocol::protocol::TokenUsageInfo;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde_json::Value as JsonValue;
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

const TOOL_NAME: &str = "clodex_usage";
const USAGE_WRAPPER: &str = "/home/tragdate/bin/codex-usage";
const WRAPPER_TIMEOUT: Duration = Duration::from_secs(10);
const WRAPPER_MAX_CHARS: usize = 2000;

struct ClodexUsageOutput(String);

impl ToolOutput for ClodexUsageOutput {
    fn log_output(&self) -> String {
        self.0.clone()
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        FunctionToolOutput::from_text(self.0.clone(), Some(true)).to_response_item(call_id, payload)
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        json!({ "usage": self.0 })
    }
}

pub struct ClodexUsageHandler;

impl ToolExecutor<ToolInvocation> for ClodexUsageHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Report usage: Anthropic rate limits last seen, this thread's token usage, and ChatGPT quota. Plain speakable text.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::object(BTreeMap::new(), None, Some(false.into())),
            output_schema: None,
        })
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(async move {
            if !matches!(invocation.payload, ToolPayload::Function { .. }) {
                return Err(FunctionCallError::RespondToModel(format!(
                    "{TOOL_NAME} handler received unsupported payload"
                )));
            }
            let (token_info, rate_limits) =
                invocation.session.token_info_and_rate_limits().await;
            let mut out = String::new();
            out.push_str(&render_rate_limits(rate_limits.as_ref()));
            out.push('\n');
            out.push_str(&render_tokens(token_info.as_ref()));
            out.push('\n');
            out.push_str(&run_usage_wrapper().await);
            Ok(boxed_tool_output(ClodexUsageOutput(out)))
        })
    }
}

impl CoreToolRuntime for ClodexUsageHandler {
    fn is_builtin_control_tool(&self) -> bool {
        true
    }
}

fn render_window(label: &str, window: &RateLimitWindow) -> String {
    let mut s = format!("{label}: {:.0}% used", window.used_percent);
    if let Some(minutes) = window.window_minutes {
        let _ = write!(s, " of a {}h window", minutes / 60);
    }
    if let Some(resets_at) = window.resets_at {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let left = (resets_at - now).max(0);
        let _ = write!(s, ", resets in {}h {}m", left / 3600, (left % 3600) / 60);
    }
    s
}

fn render_rate_limits(snapshot: Option<&RateLimitSnapshot>) -> String {
    let Some(snapshot) = snapshot else {
        return "Anthropic rate limits: none seen yet this session.".to_string();
    };
    let mut parts = Vec::new();
    if let Some(primary) = &snapshot.primary {
        parts.push(render_window("primary", primary));
    }
    if let Some(secondary) = &snapshot.secondary {
        parts.push(render_window("secondary", secondary));
    }
    if parts.is_empty() {
        return "Anthropic rate limits: snapshot seen but no windows reported.".to_string();
    }
    format!("Anthropic rate limits: {}.", parts.join("; "))
}

fn render_tokens(info: Option<&TokenUsageInfo>) -> String {
    let Some(info) = info else {
        return "Thread tokens: no usage recorded yet.".to_string();
    };
    let total = &info.total_token_usage;
    let mut s = format!(
        "Thread tokens: {} total ({} in, {} out), context now {}",
        total.total_tokens,
        total.input_tokens,
        total.output_tokens,
        info.last_token_usage.tokens_in_context_window()
    );
    if let Some(window) = info.model_context_window {
        let _ = write!(s, " of {window}");
    }
    s.push('.');
    s
}

async fn run_usage_wrapper() -> String {
    let child = tokio::process::Command::new(USAGE_WRAPPER)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(WRAPPER_TIMEOUT, child).await {
        Err(_) => "ChatGPT usage: codex-usage timed out after 10 seconds.".to_string(),
        Ok(Err(err)) => format!("ChatGPT usage: could not run codex-usage ({err})."),
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let trimmed: String = stdout.trim().chars().take(WRAPPER_MAX_CHARS).collect();
            if output.status.success() && !trimmed.is_empty() {
                format!("ChatGPT usage:\n{trimmed}")
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stderr: String = stderr.trim().chars().take(300).collect();
                format!(
                    "ChatGPT usage: codex-usage failed (status {}). {stderr}",
                    output.status
                )
            }
        }
    }
}

