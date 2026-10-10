//! Built-in catalog served when the provider speaks the Anthropic wire.

use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::config_types::Verbosity;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelMessages;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::ThinkingMode;
use codex_protocol::openai_models::ThinkingMode::Adaptive;
use codex_protocol::openai_models::ThinkingMode::Budget;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::WebSearchToolType;
use codex_protocol::protocol::MultiAgentVersion;

const CLAUDE_INSTRUCTIONS: &str = include_str!("claude_prompt.md");

/// Clodex dual-model mode: "chat+build" slugs. The main thread talks as `chat`, every
/// spawned agent without an explicit model builds as `build`.
pub const COMBO_SEPARATOR: char = '+';

/// chat slug, build slug, display name.
const COMBOS: &[(&str, &str, &str)] = &[
    ("claude-sonnet-5-5", "claude-opus-5-5", "Sonnet / Opus"),
    ("claude-opus-5-5", "claude-fable-5-1", "Opus / Fable"),
    ("claude-sonnet-5-5", "claude-fable-5-1", "Sonnet / Fable"),
    ("claude-opus-5-5", "claude-opus-5-5", "Opus / Opus"),
];

/// Splits a combo slug into (chat_model, build_model). Plain slugs return None.
pub fn combo_models(slug: &str) -> Option<(&str, &str)> {
    let (chat, build) = slug.split_once(COMBO_SEPARATOR)?;
    (!chat.is_empty() && !build.is_empty()).then_some((chat, build))
}

/// The slug the Anthropic API must see: the chat half of a combo, or the slug as is.
pub fn chat_model(slug: &str) -> &str {
    combo_models(slug).map_or(slug, |(chat, _)| chat)
}

/// slug, display name, thinking mode, auto-compact token limit.
const MODELS: &[(&str, &str, ThinkingMode, i64)] = &[
    ("claude-opus-5-5", "Claude Opus 5.5", Adaptive, 900_000),
    ("claude-opus-5", "Claude Opus 5", Adaptive, 900_000),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5", Adaptive, 900_000),
    ("claude-sonnet-5", "Claude Sonnet 5", Adaptive, 900_000),
    ("claude-haiku-5-5", "Claude Haiku 5.5", Adaptive, 900_000),
    ("claude-fable-5-1", "Claude Fable 5.1", Adaptive, 900_000),
    ("claude-fable-5", "Claude Fable 5", Adaptive, 900_000),
    ("claude-opus-4-8", "Claude Opus 4.8", Adaptive, 900_000),
    ("claude-opus-4-7", "Claude Opus 4.7", Adaptive, 900_000),
    ("claude-opus-4-6", "Claude Opus 4.6", Budget, 900_000),
    (
        "claude-opus-4-5-20251101",
        "Claude Opus 4.5",
        Budget,
        180_000,
    ),
    ("claude-sonnet-4-6", "Claude Sonnet 4.6", Budget, 900_000),
    (
        "claude-sonnet-4-5-20250929",
        "Claude Sonnet 4.5",
        Budget,
        200_000,
    ),
    ("claude-haiku-4-5", "Claude Haiku 4.5", Budget, 180_000),
    (
        "claude-haiku-4-5-20251001",
        "Claude Haiku 4.5 (20251001)",
        Budget,
        180_000,
    ),
];

pub fn claude_models_response() -> ModelsResponse {
    ModelsResponse {
        models: MODELS
            .iter()
            .map(|&(slug, display_name, thinking_mode, auto_compact)| {
                claude_model(slug, display_name, thinking_mode, auto_compact)
            })
            .chain(COMBOS.iter().map(|&(chat, build, display_name)| {
                claude_model(
                    &format!("{chat}{COMBO_SEPARATOR}{build}"),
                    display_name,
                    Adaptive,
                    900_000,
                )
            }))
            .collect(),
    }
}

fn reasoning_levels() -> Vec<ReasoningEffortPreset> {
    [
        (
            ReasoningEffort::Low,
            "Fast responses with lighter reasoning",
        ),
        (
            ReasoningEffort::Medium,
            "Balances speed and reasoning depth for everyday tasks",
        ),
        (
            ReasoningEffort::High,
            "Greater reasoning depth for complex problems",
        ),
        (
            ReasoningEffort::XHigh,
            "Extra high reasoning depth for complex problems",
        ),
        (
            ReasoningEffort::Max,
            "Maximum reasoning depth for the hardest problems",
        ),
    ]
    .into_iter()
    .map(|(effort, description)| ReasoningEffortPreset {
        effort,
        description: description.to_string(),
    })
    .collect()
}

fn claude_model(
    slug: &str,
    display_name: &str,
    thinking_mode: ThinkingMode,
    auto_compact: i64,
) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: display_name.to_string(),
        description: None,
        default_reasoning_level: Some(ReasoningEffort::Medium),
        supported_reasoning_levels: reasoning_levels(),
        shell_type: ConfigShellToolType::UnifiedExec,
        visibility: ModelVisibility::List,
        supported_in_api: true,
        priority: 5,
        additional_speed_tiers: Vec::new(),
        service_tiers: Vec::new(),
        default_service_tier: None,
        available_access_programs: None,
        availability_nux: None,
        upgrade: None,
        model_messages: Some(ModelMessages {
            instructions_template: Some(CLAUDE_INSTRUCTIONS.to_string()),
            ..Default::default()
        }),
        include_skills_usage_instructions: false,
        include_plugin_usage_instructions: false,
        include_apps_usage_instructions: false,
        supports_reasoning_summary_parameter: true,
        default_reasoning_summary: ReasoningSummary::None,
        support_verbosity: true,
        default_verbosity: Some(Verbosity::Low),
        apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
        web_search_tool_type: WebSearchToolType::TextAndImage,
        truncation_policy: TruncationPolicyConfig::tokens(/*limit*/ 10_000),
        supports_image_detail_original: true,
        context_window: Some(1_000_000),
        max_context_window: Some(1_000_000),
        auto_compact_token_limit: Some(auto_compact),
        comp_hash: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: vec![
            "send_user_message_async".to_string(),
            "clock".to_string(),
        ],
        input_modalities: vec![InputModality::Text, InputModality::Image],
        used_fallback_model_metadata: false,
        supports_search_tool: true,
        supports_experimental_context: false,
        use_responses_lite: false,
        supports_reasoning_effort_updates: true,
        guardian: None,
        node_repl_auto_review_required: true,
        node_repl_disabled: false,
        auto_review_model_override: None,
        model_specialty: None,
        tool_mode: Some(ToolMode::CodeModeOnly),
        multi_agent_version: Some(MultiAgentVersion::V2),
        multi_agent_reasoning_effort: None,
        thinking_mode: Some(thinking_mode),
    }
}
