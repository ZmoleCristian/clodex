use crate::endpoint::realtime_websocket::methods_common::REALTIME_AUDIO_SAMPLE_RATE;
use crate::endpoint::realtime_websocket::protocol::AudioFormatType;
use crate::endpoint::realtime_websocket::protocol::ConversationContentType;
use crate::endpoint::realtime_websocket::protocol::ConversationItemContent;
use crate::endpoint::realtime_websocket::protocol::ConversationItemPayload;
use crate::endpoint::realtime_websocket::protocol::ConversationItemType;
use crate::endpoint::realtime_websocket::protocol::ConversationMessageItem;
use crate::endpoint::realtime_websocket::protocol::RealtimeOutboundMessage;
use crate::endpoint::realtime_websocket::protocol::RealtimeVoice;
use crate::endpoint::realtime_websocket::protocol::SessionAudio;
use crate::endpoint::realtime_websocket::protocol::SessionAudioFormat;
use crate::endpoint::realtime_websocket::protocol::SessionAudioInput;
use crate::endpoint::realtime_websocket::protocol::SessionAudioOutput;
use crate::endpoint::realtime_websocket::protocol::SessionFunctionTool;
use crate::endpoint::realtime_websocket::protocol::SessionToolType;
use crate::endpoint::realtime_websocket::protocol::SessionType;
use crate::endpoint::realtime_websocket::protocol::SessionUpdateSession;
use codex_protocol::protocol::ConversationTextRole;
use serde_json::json;

// Clodex: the desktop voice call runs on v1; advertise the interrupt tool there too.
const REALTIME_V1_INTERRUPT_TOOL_NAME: &str = "interrupt_background_agent";
const REALTIME_V1_INTERRUPT_TOOL_DESCRIPTION: &str = "Call this when the user asks to stop, cancel or abort the current work. It interrupts the background agent's running task immediately; a later request starts a fresh task. This does not end the voice call.";

pub(super) fn conversation_item_create_message(
    text: String,
    role: ConversationTextRole,
) -> RealtimeOutboundMessage {
    let content_type = match role {
        ConversationTextRole::Assistant => ConversationContentType::OutputText,
        ConversationTextRole::User | ConversationTextRole::Developer => {
            ConversationContentType::InputText
        }
    };

    RealtimeOutboundMessage::ConversationItemCreate {
        item: ConversationItemPayload::Message(ConversationMessageItem {
            r#type: ConversationItemType::Message,
            role,
            content: vec![ConversationItemContent {
                r#type: content_type,
                text,
            }],
        }),
    }
}

pub(super) fn conversation_handoff_append_message(
    handoff_id: String,
    output_text: String,
) -> RealtimeOutboundMessage {
    RealtimeOutboundMessage::ConversationHandoffAppend {
        handoff_id,
        output_text,
    }
}

pub(super) fn session_update_session(
    instructions: String,
    voice: RealtimeVoice,
) -> SessionUpdateSession {
    SessionUpdateSession {
        id: None,
        r#type: SessionType::Quicksilver,
        model: None,
        instructions: Some(instructions),
        output_modalities: None,
        audio: SessionAudio {
            input: SessionAudioInput {
                format: SessionAudioFormat {
                    r#type: AudioFormatType::AudioPcm,
                    rate: REALTIME_AUDIO_SAMPLE_RATE,
                },
                noise_reduction: None,
                transcription: None,
                turn_detection: None,
            },
            output: Some(SessionAudioOutput {
                format: None,
                voice,
            }),
        },
        tools: Some(vec![SessionFunctionTool {
            r#type: SessionToolType::Function,
            name: REALTIME_V1_INTERRUPT_TOOL_NAME.to_string(),
            description: REALTIME_V1_INTERRUPT_TOOL_DESCRIPTION.to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        }]),
        tool_choice: Some("auto".to_string()),
    }
}

pub(super) fn websocket_intent() -> Option<&'static str> {
    Some("quicksilver")
}
