//! Codex prompt -> Anthropic Messages request body.

use crate::CLAUDE_CODE_SYSTEM_INSTRUCTION;
use crate::TOOL_PREFIX;
use codex_protocol::DEFAULT_FUNCTION_NAMESPACE;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::WebSearchAction;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ThinkingMode;
use codex_tools::FreeformTool;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::collections::HashSet;

/// Marker stored in \`Reasoning.encrypted_content\` for \`redacted_thinking\` blocks.
pub(crate) const REDACTED_THINKING_PREFIX: &str = "redacted:";

const DEFAULT_MAX_TOKENS: u64 = 8192;
const OUTPUT_HEADROOM: u64 = 8192;
const OUTPUT_FLOOR: u64 = 4096;
const MIN_THINKING_BUDGET: u64 = 1024;
const BUDGET_THINKING_CEILING: u64 = 32_000;
const ADAPTIVE_THINKING_CEILING: u64 = 40_192;
const WEB_SEARCH_MAX_USES: u32 = 5;

pub struct RequestInput<'a> {
    pub model: &'a str,
    /// From the model catalog; unknown slugs fall back to a fixed budget.
    pub thinking_mode: Option<ThinkingMode>,
    pub instructions: &'a str,
    pub input: &'a [ResponseItem],
    pub tools: &'a [ToolSpec],
    pub effort: Option<ReasoningEffort>,
    pub parallel_tool_calls: bool,
    pub output_schema: Option<&'a Value>,
}

/// Codex identity of a tool as declared on the wire.
#[derive(Clone, Debug)]
pub struct WireTool {
    pub namespace: Option<String>,
    pub name: String,
    pub freeform: bool,
}

pub type ToolNameMap = HashMap<String, WireTool>;

pub struct MessagesRequest {
    pub body: Value,
    /// Wire tool name -> Codex tool identity, for mapping \`tool_use\` back.
    pub tool_names: ToolNameMap,
}

pub fn build_request(input: RequestInput<'_>) -> MessagesRequest {
    let mut tool_names = ToolNameMap::new();
    let mut tools = tools_json(input.tools, &mut tool_names);
    let messages = messages_json(input.input);

    let mut system = vec![json!({"type": "text", "text": CLAUDE_CODE_SYSTEM_INSTRUCTION})];
    if !input.instructions.trim().is_empty() {
        system.push(json!({"type": "text", "text": input.instructions}));
    }
    let cache = json!({"type": "ephemeral"});
    if let Some(last) = system.last_mut() {
        last["cache_control"] = cache.clone();
    }
    if let Some(last) = tools.last_mut() {
        last["cache_control"] = cache.clone();
    }

    let mut body = Map::new();
    body.insert("model".into(), json!(input.model));
    body.insert("stream".into(), json!(true));
    body.insert("system".into(), Value::Array(system));
    body.insert("messages".into(), Value::Array(messages));
    body.insert("cache_control".into(), cache);
    if !tools.is_empty() {
        body.insert("tools".into(), Value::Array(tools));
        if !input.parallel_tool_calls {
            body.insert(
                "tool_choice".into(),
                json!({"type": "auto", "disable_parallel_tool_use": true}),
            );
        }
    }
    apply_thinking(&mut body, input.thinking_mode, input.effort);
    if let Some(schema) = input.output_schema {
        body.entry("output_config")
            .or_insert_with(|| json!({}))["format"] = json!({"type": "json_schema", "schema": schema});
    }

    MessagesRequest {
        body: Value::Object(body),
        tool_names,
    }
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

pub(crate) fn wire_tool_name(namespace: Option<&str>, name: &str) -> String {
    match namespace {
        Some(ns) if ns != DEFAULT_FUNCTION_NAMESPACE => format!("{TOOL_PREFIX}{ns}__{name}"),
        _ => format!("{TOOL_PREFIX}{name}"),
    }
}

fn tools_json(tools: &[ToolSpec], names: &mut ToolNameMap) -> Vec<Value> {
    let mut out = Vec::new();
    for spec in tools {
        match spec {
            ToolSpec::Function(tool) => out.push(function_tool(None, tool, names)),
            ToolSpec::Freeform(tool) => out.push(freeform_tool(None, tool, names)),
            ToolSpec::Namespace(namespace) => {
                let ns = Some(namespace.name.as_str());
                for tool in &namespace.tools {
                    match tool {
                        ResponsesApiNamespaceTool::Function(tool) => {
                            out.push(function_tool(ns, tool, names));
                        }
                        ResponsesApiNamespaceTool::Custom(tool) => {
                            out.push(freeform_tool(ns, tool, names));
                        }
                    }
                }
            }
            ToolSpec::WebSearch { .. } => out.push(json!({
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": WEB_SEARCH_MAX_USES,
            })),
            ToolSpec::ToolSearch { .. } => {}
        }
    }
    out
}

fn register(ns: Option<&str>, name: &str, freeform: bool, names: &mut ToolNameMap) -> String {
    let wire = wire_tool_name(ns, name);
    names.insert(
        wire.clone(),
        WireTool {
            namespace: ns
                .filter(|ns| *ns != DEFAULT_FUNCTION_NAMESPACE)
                .map(str::to_string),
            name: name.to_string(),
            freeform,
        },
    );
    wire
}

fn function_tool(ns: Option<&str>, tool: &ResponsesApiTool, names: &mut ToolNameMap) -> Value {
    let wire = register(ns, &tool.name, /*freeform*/ false, names);
    let schema = serde_json::to_value(&tool.parameters).unwrap_or(Value::Null);
    json!({
        "name": wire,
        "description": tool.description,
        "input_schema": input_schema(schema),
    })
}

fn freeform_tool(ns: Option<&str>, tool: &FreeformTool, names: &mut ToolNameMap) -> Value {
    let wire = register(ns, &tool.name, /*freeform*/ true, names);
    json!({
        "name": wire,
        "description": tool.description,
        "input_schema": {
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": format!(
                        "Raw tool input ({} syntax):\n{}",
                        tool.format.syntax, tool.format.definition
                    ),
                }
            },
            "required": ["input"],
        },
    })
}

/// Anthropic rejects a root without \`type: object\` and root-level oneOf/anyOf/allOf.
fn input_schema(schema: Value) -> Value {
    let mut obj = match schema {
        Value::Object(obj) => obj,
        _ => Map::new(),
    };
    obj.remove("encrypted");
    let mut properties = match obj.remove("properties") {
        Some(Value::Object(props)) => props,
        _ => Map::new(),
    };
    let mut required: Vec<String> = match obj.remove("required") {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    for key in ["oneOf", "anyOf", "allOf"] {
        let Some(Value::Array(variants)) = obj.remove(key) else {
            continue;
        };
        for variant in variants {
            let Value::Object(variant) = variant else {
                continue;
            };
            if let Some(Value::Object(props)) = variant.get("properties") {
                properties.extend(props.clone());
            }
            if key == "allOf"
                && let Some(Value::Array(items)) = variant.get("required")
            {
                required.extend(items.iter().filter_map(|v| v.as_str().map(str::to_string)));
            }
        }
    }
    let mut seen = HashSet::new();
    required.retain(|name| seen.insert(name.clone()));
    obj.insert("type".into(), json!("object"));
    obj.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        obj.insert("required".into(), json!(required));
    }
    Value::Object(obj)
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

struct Messages(Vec<Value>);

impl Messages {
    fn push(&mut self, role: &str, block: Value) {
        if let Some(last) = self.0.last_mut()
            && last["role"] == role
            && let Some(content) = last["content"].as_array_mut()
        {
            content.push(block);
            return;
        }
        self.0.push(json!({"role": role, "content": [block]}));
    }
}

fn messages_json(items: &[ResponseItem]) -> Vec<Value> {
    let mut messages = Messages(Vec::new());
    for item in items {
        match item {
            ResponseItem::Message { role, content, .. } => {
                let role = if role == "assistant" { "assistant" } else { "user" };
                for part in content {
                    if let Some(block) = content_block(part) {
                        messages.push(role, block);
                    }
                }
            }
            ResponseItem::Reasoning {
                summary,
                content,
                encrypted_content: Some(signature),
                ..
            } => {
                if let Some(data) = signature.strip_prefix(REDACTED_THINKING_PREFIX) {
                    messages.push(
                        "assistant",
                        json!({"type": "redacted_thinking", "data": data}),
                    );
                } else if signature.contains(':') || signature.starts_with("gAAAAA") {
                    // Not an Anthropic signature: an opencodex-wrapped "ocxr1:…" or an OpenAI
                    // Fernet blob ("gAAAAA…") from a thread that ran on a Responses provider.
                    // Anthropic rejects both; drop the thinking block, keep the rest of history.
                    continue;
                } else {
                    let mut text: Vec<&str> = summary
                        .iter()
                        .map(|ReasoningItemReasoningSummary::SummaryText { text }| text.as_str())
                        .collect();
                    if text.is_empty()
                        && let Some(content) = content
                    {
                        text = content
                            .iter()
                            .map(|part| match part {
                                ReasoningItemContent::ReasoningText { text }
                                | ReasoningItemContent::Text { text } => text.as_str(),
                            })
                            .collect();
                    }
                    messages.push(
                        "assistant",
                        json!({"type": "thinking", "thinking": text.join(""), "signature": signature}),
                    );
                }
            }
            ResponseItem::FunctionCall {
                name,
                namespace,
                arguments,
                call_id,
                ..
            } => {
                let input = serde_json::from_str::<Value>(arguments)
                    .ok()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({}));
                messages.push(
                    "assistant",
                    json!({
                        "type": "tool_use",
                        "id": call_id,
                        "name": wire_tool_name(namespace.as_deref(), name),
                        "input": input,
                    }),
                );
            }
            ResponseItem::CustomToolCall {
                call_id,
                name,
                input,
                ..
            } => {
                messages.push(
                    "assistant",
                    json!({
                        "type": "tool_use",
                        "id": call_id,
                        "name": wire_tool_name(None, name),
                        "input": {"input": input},
                    }),
                );
            }
            ResponseItem::FunctionCallOutput {
                call_id: Some(call_id),
                output,
                ..
            }
            | ResponseItem::CustomToolCallOutput {
                call_id, output, ..
            } => {
                messages.push("user", tool_result(call_id, output));
            }
            // Codex delivers tool-less inputs (new-thread prompts from create_thread,
            // delegation payloads) as a FunctionCallOutput with no call_id. Anthropic has no
            // tool_result to attach it to, so it becomes plain user text.
            ResponseItem::FunctionCallOutput { call_id: None, output, .. } => {
                for block in output_blocks(output) {
                    messages.push("user", block);
                }
            }
            // Anthropic server tool calls cannot be replayed; keep the history coherent with a
            // text marker so the model still sees that it searched.
            ResponseItem::WebSearchCall { action, .. } => {
                let query = match action {
                    Some(WebSearchAction::Search { query: Some(query), .. }) => query.clone(),
                    Some(WebSearchAction::Search { queries: Some(queries), .. }) => queries.join(", "),
                    Some(WebSearchAction::OpenPage { url: Some(url) })
                    | Some(WebSearchAction::FindInPage { url: Some(url), .. }) => url.clone(),
                    _ => String::new(),
                };
                messages.push("assistant", json!({"type": "text", "text": format!("[web search: {query}]")}));
            }
            ResponseItem::Reasoning { .. }
            | ResponseItem::AdditionalTools { .. }
            | ResponseItem::AgentMessage { .. }
            | ResponseItem::LocalShellCall { .. }
            | ResponseItem::ToolSearchCall { .. }
            | ResponseItem::ToolSearchOutput { .. }
            | ResponseItem::ImageGenerationCall { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::ConfigurationUpdate { .. }
            | ResponseItem::CompactionTrigger { .. }
            | ResponseItem::ContextCompaction { .. }
            | ResponseItem::Other => {}
        }
    }
    let mut messages = messages.0;
    repair_tool_pairing(&mut messages);
    if messages.first().is_none_or(|m| m["role"] != "user") {
        messages.insert(0, json!({"role": "user", "content": "(continue)"}));
    }
    if messages.last().is_some_and(|m| m["role"] == "assistant") {
        messages.push(json!({"role": "user", "content": "(continue)"}));
    }
    messages
}

fn content_block(part: &ContentItem) -> Option<Value> {
    match part {
        ContentItem::InputText { text } | ContentItem::OutputText { text } => text_block(text),
        ContentItem::InputImage { image, .. } => image_block(image),
        ContentItem::InputAudio { .. } => None,
    }
}

fn text_block(text: &str) -> Option<Value> {
    (!text.is_empty()).then(|| json!({"type": "text", "text": text}))
}

fn image_block(image: &ImageReference) -> Option<Value> {
    let ImageReference::Inline { image_url } = image else {
        return None;
    };
    let Some(rest) = image_url.strip_prefix("data:") else {
        return Some(json!({"type": "image", "source": {"type": "url", "url": image_url}}));
    };
    let (media_type, data) = rest.split_once(";base64,")?;
    Some(json!({
        "type": "image",
        "source": {"type": "base64", "media_type": media_type, "data": data},
    }))
}

fn tool_result(call_id: &str, output: &FunctionCallOutputPayload) -> Value {
    let blocks = output_blocks(output);
    let content = if blocks.is_empty() {
        json!("(empty tool output)")
    } else {
        Value::Array(blocks)
    };
    let mut block = json!({"type": "tool_result", "tool_use_id": call_id, "content": content});
    if output.success == Some(false) {
        block["is_error"] = json!(true);
    }
    block
}

/// Content blocks for a tool output payload; empty when there is nothing renderable.
fn output_blocks(output: &FunctionCallOutputPayload) -> Vec<Value> {
    match &output.body {
        FunctionCallOutputBody::Text(text) => text_block(text).into_iter().collect(),
        FunctionCallOutputBody::ContentItems(items) => items
            .iter()
            .filter_map(|item| match item {
                FunctionCallOutputContentItem::InputText { text } => text_block(text),
                FunctionCallOutputContentItem::InputImage { image, .. } => image_block(image),
                FunctionCallOutputContentItem::InputAudio { .. }
                | FunctionCallOutputContentItem::EncryptedContent { .. } => None,
            })
            .collect(),
    }
}

/// Every \`tool_use\` must be answered by a \`tool_result\` at the head of the next user message,
/// and a \`tool_result\` without its \`tool_use\` is invalid; fix both shapes.
fn repair_tool_pairing(messages: &mut Vec<Value>) {
    let mut i = 0;
    while i < messages.len() {
        if messages[i]["role"] == "user" {
            pair_results(&mut messages[i], &[]);
            i += 1;
            continue;
        }
        let expected: Vec<String> = messages[i]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "tool_use")
            .filter_map(|b| b["id"].as_str().map(str::to_string))
            .collect();
        i += 1;
        if expected.is_empty() {
            continue;
        }
        if messages.get(i).is_none_or(|m| m["role"] != "user") {
            messages.insert(i, json!({"role": "user", "content": []}));
        }
        pair_results(&mut messages[i], &expected);
        i += 1;
    }
}

fn pair_results(message: &mut Value, expected: &[String]) {
    let Some(content) = message["content"].as_array_mut() else {
        return;
    };
    let mut results = Vec::new();
    let mut rest = Vec::new();
    let mut seen = HashSet::new();
    for block in content.drain(..) {
        match block["tool_use_id"].as_str().map(str::to_string) {
            Some(id) if expected.contains(&id) && !seen.contains(&id) => {
                seen.insert(id);
                results.push(block);
            }
            Some(id) => rest.push(json!({
                "type": "text",
                "text": format!("[tool_result without adjacent tool_use: {id}]\n{}", block["content"]),
            })),
            None => rest.push(block),
        }
    }
    for id in expected {
        if !seen.contains(id) {
            results.push(json!({
                "type": "tool_result",
                "tool_use_id": id,
                "content": "[missing tool_result for this tool_use in history]",
                "is_error": true,
            }));
        }
    }
    results.extend(rest);
    *content = results;
}

// ---------------------------------------------------------------------------
// Thinking
// ---------------------------------------------------------------------------

fn thinking_budget(effort: &str) -> u64 {
    match effort {
        "minimal" => 1024,
        "low" => 4096,
        "high" => 16384,
        "xhigh" => 24576,
        "max" | "ultra" => 32000,
        _ => 8192,
    }
}

fn apply_thinking(
    body: &mut Map<String, Value>,
    thinking_mode: Option<ThinkingMode>,
    effort: Option<ReasoningEffort>,
) {
    let effort = effort.unwrap_or(ReasoningEffort::Medium);
    let effort = match effort {
        ReasoningEffort::None => {
            body.insert("max_tokens".into(), json!(DEFAULT_MAX_TOKENS));
            return;
        }
        ReasoningEffort::Ultra => "max".to_string(),
        ReasoningEffort::Persistent => "high".to_string(),
        other => other.as_str().to_string(),
    };
    let budget = thinking_budget(&effort);
    if thinking_mode == Some(ThinkingMode::Adaptive) {
        let effort = if effort == "minimal" { "low" } else { effort.as_str() };
        body.insert(
            "thinking".into(),
            json!({"type": "adaptive", "display": "summarized"}),
        );
        body.insert("output_config".into(), json!({"effort": effort}));
        body.insert(
            "max_tokens".into(),
            json!((budget + OUTPUT_HEADROOM).clamp(DEFAULT_MAX_TOKENS, ADAPTIVE_THINKING_CEILING)),
        );
    } else {
        let max_tokens = (budget + OUTPUT_HEADROOM).min(BUDGET_THINKING_CEILING);
        let budget = budget.min(max_tokens - OUTPUT_FLOOR).max(MIN_THINKING_BUDGET);
        body.insert("max_tokens".into(), json!(max_tokens));
        body.insert(
            "thinking".into(),
            json!({"type": "enabled", "budget_tokens": budget}),
        );
    }
}
