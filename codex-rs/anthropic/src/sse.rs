//! Anthropic Messages SSE -> \`ResponseEvent\`.

use crate::TOOL_PREFIX;
use crate::request::REDACTED_THINKING_PREFIX;
use crate::request::ToolNameMap;
use codex_api::ApiError;
use codex_api::ResponseEvent;
use codex_api::ResponseStream;
use codex_http_client::StreamResponse;
use codex_protocol::ResponseItemId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::WebSearchAction;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::debug;
use tracing::trace;

pub fn spawn_response_stream(
    response: StreamResponse,
    idle_timeout: Duration,
    tool_names: ToolNameMap,
) -> ResponseStream {
    let upstream_request_id = response
        .headers
        .get("request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent, ApiError>>(1600);
    let rate_limits = crate::rate_limits::parse_rate_limits(&response.headers);
    tokio::spawn(async move {
        if let Some(snapshot) = rate_limits
            && tx_event.send(Ok(ResponseEvent::RateLimits(snapshot))).await.is_err()
        {
            return;
        }
        let mut stream = response
            .bytes
            .map(|chunk| chunk.map_err(|e| e.to_string()))
            .eventsource();
        let mut state = State::new(tool_names);
        loop {
            let event = match timeout(idle_timeout, stream.next()).await {
                Ok(Some(Ok(event))) => event,
                Ok(Some(Err(err))) => {
                    let _ = tx_event.send(Err(ApiError::Stream(err.to_string()))).await;
                    return;
                }
                Ok(None) => {
                    let _ = tx_event
                        .send(Err(ApiError::Stream(
                            "stream closed before message_stop".into(),
                        )))
                        .await;
                    return;
                }
                Err(_) => {
                    let _ = tx_event
                        .send(Err(ApiError::Stream("idle timeout waiting for SSE".into())))
                        .await;
                    return;
                }
            };
            let frame: Frame = match serde_json::from_str(&event.data) {
                Ok(frame) => frame,
                Err(err) => {
                    debug!("dropping unparseable anthropic frame: {err}");
                    continue;
                }
            };
            trace!(kind = %frame.kind, data = %event.data, "anthropic sse frame");
            let mut events = Vec::new();
            let done = state.handle(frame, &mut events);
            for event in events {
                let is_err = event.is_err();
                if tx_event.send(event).await.is_err() || is_err {
                    return;
                }
            }
            if done {
                return;
            }
        }
    });
    ResponseStream {
        rx_event,
        upstream_request_id,
        interrupt: None,
    }
}

#[derive(Deserialize)]
struct Frame {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    message: Option<Value>,
    #[serde(default)]
    content_block: Option<Value>,
    #[serde(default)]
    delta: Option<Value>,
    #[serde(default)]
    usage: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Default)]
struct Usage {
    input: i64,
    cache_read: i64,
    cache_write: i64,
    output: i64,
}

impl Usage {
    fn merge(&mut self, usage: &Value) {
        let get = |key: &str, current: i64| usage.get(key).and_then(Value::as_i64).unwrap_or(current);
        self.input = get("input_tokens", self.input);
        self.cache_read = get("cache_read_input_tokens", self.cache_read);
        self.cache_write = get("cache_creation_input_tokens", self.cache_write);
        self.output = get("output_tokens", self.output);
    }

    fn token_usage(&self) -> TokenUsage {
        let input_tokens = self.input + self.cache_read + self.cache_write;
        TokenUsage {
            input_tokens,
            cached_input_tokens: self.cache_read,
            cache_write_input_tokens: self.cache_write,
            output_tokens: self.output,
            reasoning_output_tokens: 0,
            total_tokens: input_tokens + self.output,
            codex_rollout_budget_units: None,
        }
    }
}

enum Block {
    None,
    Text {
        text: String,
        /// URLs cited by this block, in first-seen order.
        citations: Vec<String>,
    },
    Thinking {
        text: String,
        signature: Option<String>,
    },
    Redacted(String),
    ToolUse {
        id: String,
        name: String,
        namespace: Option<String>,
        freeform: bool,
        json: String,
    },
    WebSearch {
        id: String,
        json: String,
    },
}

struct State {
    tool_names: ToolNameMap,
    response_id: String,
    usage: Usage,
    stop_reason: Option<String>,
    block: Block,
    /// Finished text waiting to see whether the next block continues it. Anthropic splits a
    /// cited answer into one `text` block per citation span; Codex wants one message.
    pending_text: Option<String>,
    /// Unique cited URLs of the message being built; position + 1 is the `[[n]]` marker.
    sources: Vec<String>,
}

impl State {
    fn new(tool_names: ToolNameMap) -> Self {
        Self {
            tool_names,
            response_id: String::new(),
            usage: Usage::default(),
            stop_reason: None,
            block: Block::None,
            pending_text: None,
            sources: Vec::new(),
        }
    }

    /// Returns true when the stream reached its terminal frame.
    fn handle(&mut self, frame: Frame, out: &mut Vec<Result<ResponseEvent, ApiError>>) -> bool {
        match frame.kind.as_str() {
            "message_start" => {
                if let Some(message) = &frame.message {
                    self.response_id = message["id"].as_str().unwrap_or_default().to_string();
                    if let Some(usage) = message.get("usage") {
                        self.usage.merge(usage);
                    }
                }
                out.push(Ok(ResponseEvent::Created {
                    response_id: Some(self.response_id.clone()),
                }));
            }
            "content_block_start" => {
                let Some(block) = frame.content_block else {
                    return false;
                };
                self.start_block(&block, out);
            }
            "content_block_delta" => {
                let Some(delta) = frame.delta else {
                    return false;
                };
                self.delta(&delta, out);
            }
            "content_block_stop" => {
                let block = std::mem::replace(&mut self.block, Block::None);
                self.stop_block(block, out);
            }
            "message_delta" => {
                if let Some(usage) = &frame.usage {
                    self.usage.merge(usage);
                }
                if let Some(reason) = frame
                    .delta
                    .as_ref()
                    .and_then(|d| d["stop_reason"].as_str())
                {
                    self.stop_reason = Some(reason.to_string());
                }
            }
            "message_stop" => {
                self.flush_text(out);
                out.push(self.completed());
                return true;
            }
            "error" => {
                let message = frame
                    .error
                    .as_ref()
                    .and_then(|e| e["message"].as_str())
                    .unwrap_or("anthropic stream error")
                    .to_string();
                out.push(Err(ApiError::Stream(message)));
                return true;
            }
            _ => {}
        }
        false
    }

    fn start_block(&mut self, block: &Value, out: &mut Vec<Result<ResponseEvent, ApiError>>) {
        let kind = block["type"].as_str().unwrap_or_default();
        if kind != "text" {
            self.flush_text(out);
        }
        match kind {
            "text" => {
                let text = match self.pending_text.take() {
                    Some(text) => text,
                    None => {
                        out.push(Ok(ResponseEvent::OutputItemAdded(message_item(String::new()))));
                        String::new()
                    }
                };
                self.block = Block::Text {
                    text,
                    citations: Vec::new(),
                };
            }
            "thinking" => {
                self.block = Block::Thinking {
                    text: String::new(),
                    signature: None,
                };
                out.push(Ok(ResponseEvent::OutputItemAdded(reasoning_item(
                    String::new(),
                    None,
                ))));
                out.push(Ok(ResponseEvent::ReasoningSummaryPartAdded { summary_index: 0 }));
            }
            "redacted_thinking" => {
                self.block = Block::Redacted(block["data"].as_str().unwrap_or_default().to_string());
            }
            "tool_use" => {
                let id = block["id"].as_str().unwrap_or_default().to_string();
                let wire = block["name"].as_str().unwrap_or_default();
                let (name, namespace, freeform) = match self.tool_names.get(wire) {
                    Some(tool) => (tool.name.clone(), tool.namespace.clone(), tool.freeform),
                    None => (
                        wire.strip_prefix(TOOL_PREFIX).unwrap_or(wire).to_string(),
                        None,
                        false,
                    ),
                };
                let item = if freeform {
                    ResponseItem::CustomToolCall {
                        id: None,
                        status: None,
                        call_id: id.clone(),
                        name: name.clone(),
                        namespace: namespace.clone(),
                        input: String::new(),
                        internal_chat_message_metadata_passthrough: None,
                    }
                } else {
                    ResponseItem::FunctionCall {
                        id: None,
                        name: name.clone(),
                        namespace: namespace.clone(),
                        arguments: String::new(),
                        encrypted_function_args: None,
                        call_id: id.clone(),
                        internal_chat_message_metadata_passthrough: None,
                    }
                };
                out.push(Ok(ResponseEvent::OutputItemAdded(item)));
                self.block = Block::ToolUse {
                    id,
                    name,
                    namespace,
                    freeform,
                    json: String::new(),
                };
            }
            "server_tool_use" if block["name"] == "web_search" => {
                let id = block["id"].as_str().unwrap_or_default().to_string();
                out.push(Ok(ResponseEvent::OutputItemAdded(web_search_item(
                    id.clone(),
                    "in_progress",
                    None,
                ))));
                self.block = Block::WebSearch {
                    id,
                    json: String::new(),
                };
            }
            // Results are consumed server-side; the cited answer streams as ordinary text.
            "web_search_tool_result" => {}
            other => debug!("ignoring anthropic content block type {other}"),
        }
    }

    fn delta(&mut self, delta: &Value, out: &mut Vec<Result<ResponseEvent, ApiError>>) {
        match (delta["type"].as_str().unwrap_or_default(), &mut self.block) {
            ("text_delta", Block::Text { text, .. }) => {
                let piece = delta["text"].as_str().unwrap_or_default();
                text.push_str(piece);
                out.push(Ok(ResponseEvent::OutputTextDelta(piece.to_string())));
            }
            ("citations_delta", Block::Text { citations, .. }) => {
                if let Some(url) = delta["citation"]["url"].as_str()
                    && !citations.iter().any(|c| c == url)
                {
                    citations.push(url.to_string());
                }
            }
            ("thinking_delta", Block::Thinking { text, .. }) => {
                let piece = delta["thinking"].as_str().unwrap_or_default();
                text.push_str(piece);
                out.push(Ok(ResponseEvent::ReasoningSummaryDelta {
                    delta: piece.to_string(),
                    summary_index: 0,
                }));
            }
            ("signature_delta", Block::Thinking { signature, .. }) => {
                *signature = delta["signature"].as_str().map(str::to_string);
            }
            ("input_json_delta", Block::ToolUse { id, json, .. }) => {
                let piece = delta["partial_json"].as_str().unwrap_or_default();
                json.push_str(piece);
                out.push(Ok(ResponseEvent::ToolCallInputDelta {
                    item_id: id.clone(),
                    call_id: Some(id.clone()),
                    delta: piece.to_string(),
                }));
            }
            ("input_json_delta", Block::WebSearch { json, .. }) => {
                json.push_str(delta["partial_json"].as_str().unwrap_or_default());
            }
            _ => {}
        }
    }

    fn stop_block(&mut self, block: Block, out: &mut Vec<Result<ResponseEvent, ApiError>>) {
        match block {
            Block::None => {}
            Block::Text {
                mut text,
                citations,
            } => {
                let marks: String = citations
                    .iter()
                    .map(|url| format!(" [[{}]]({url})", self.source_index(url)))
                    .collect();
                if !marks.is_empty() {
                    text.insert_str(text.trim_end().len(), &marks);
                    out.push(Ok(ResponseEvent::OutputTextDelta(marks)));
                }
                self.pending_text = Some(text);
            }
            Block::Thinking { text, signature } => {
                out.push(Ok(ResponseEvent::OutputItemDone(reasoning_item(
                    text, signature,
                ))));
            }
            Block::Redacted(data) => {
                out.push(Ok(ResponseEvent::OutputItemDone(reasoning_item(
                    String::new(),
                    Some(format!("{REDACTED_THINKING_PREFIX}{data}")),
                ))));
            }
            Block::ToolUse {
                id,
                name,
                namespace,
                freeform,
                json,
            } => {
                let arguments = if json.trim().is_empty() {
                    "{}".to_string()
                } else {
                    json
                };
                let item = if freeform {
                    let input = serde_json::from_str::<Value>(&arguments)
                        .ok()
                        .and_then(|v| v["input"].as_str().map(str::to_string))
                        .unwrap_or(arguments);
                    ResponseItem::CustomToolCall {
                        id: None,
                        status: None,
                        call_id: id,
                        name,
                        namespace,
                        input,
                        internal_chat_message_metadata_passthrough: None,
                    }
                } else {
                    ResponseItem::FunctionCall {
                        id: None,
                        name,
                        namespace,
                        arguments,
                        encrypted_function_args: None,
                        call_id: id,
                        internal_chat_message_metadata_passthrough: None,
                    }
                };
                out.push(Ok(ResponseEvent::OutputItemDone(item)));
            }
            Block::WebSearch { id, json } => {
                let query = serde_json::from_str::<Value>(&json)
                    .ok()
                    .and_then(|v| v["query"].as_str().map(str::to_string));
                out.push(Ok(ResponseEvent::OutputItemDone(web_search_item(
                    id,
                    "completed",
                    query,
                ))));
            }
        }
    }

    fn flush_text(&mut self, out: &mut Vec<Result<ResponseEvent, ApiError>>) {
        if let Some(text) = self.pending_text.take() {
            out.push(Ok(ResponseEvent::OutputItemDone(message_item(text))));
        }
        self.sources.clear();
    }

    fn source_index(&mut self, url: &str) -> usize {
        match self.sources.iter().position(|s| s == url) {
            Some(i) => i + 1,
            None => {
                self.sources.push(url.to_string());
                self.sources.len()
            }
        }
    }

    fn completed(&self) -> Result<ResponseEvent, ApiError> {
        match self.stop_reason.as_deref() {
            Some("refusal") | Some("content_filter") => return Err(ApiError::ContentFilter),
            Some("error") => {
                return Err(ApiError::Stream(
                    "upstream ended the turn with stop_reason \"error\"".into(),
                ));
            }
            _ => {}
        }
        Ok(ResponseEvent::Completed {
            response_id: self.response_id.clone(),
            token_usage: Some(self.usage.token_usage()),
            usage_metadata: None,
            end_turn: Some(self.stop_reason.as_deref() == Some("end_turn")),
        })
    }
}

fn message_item(text: String) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: if text.is_empty() {
            Vec::new()
        } else {
            vec![ContentItem::OutputText { text }]
        },
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn reasoning_item(text: String, signature: Option<String>) -> ResponseItem {
    ResponseItem::Reasoning {
        id: None,
        summary: if text.is_empty() {
            Vec::new()
        } else {
            vec![ReasoningItemReasoningSummary::SummaryText { text }]
        },
        content: None,
        encrypted_content: signature,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn web_search_item(id: String, status: &str, query: Option<String>) -> ResponseItem {
    ResponseItem::WebSearchCall {
        id: Some(ResponseItemId::from_server(id)),
        status: Some(status.to_string()),
        action: query.map(|query| WebSearchAction::Search {
            query: Some(query),
            queries: None,
        }),
        internal_chat_message_metadata_passthrough: None,
    }
}
