pub const BACKEND_PROMPT: &str = include_str!("../templates/realtime/backend_prompt.md");
pub const END_INSTRUCTIONS: &str = include_str!("../templates/realtime/realtime_end.md");
pub const START_INSTRUCTIONS: &str = include_str!("../templates/realtime/realtime_start.md");
/// Clodex: rules for a Claude backend in a voice session, appended to every realtime start.
pub const CLODEX_VOICE_ADDENDUM: &str =
    include_str!("../templates/realtime/clodex_voice_addendum.md");
