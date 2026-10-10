pub const BACKEND_PROMPT: &str = include_str!("../templates/realtime/backend_prompt.md");
pub const END_INSTRUCTIONS: &str = include_str!("../templates/realtime/realtime_end.md");
pub const START_INSTRUCTIONS: &str = include_str!("../templates/realtime/realtime_start.md");
/// Clodex: rules for a Claude backend in a voice session, appended to every realtime start.
pub const CLODEX_VOICE_ADDENDUM: &str =
    include_str!("../templates/realtime/clodex_voice_addendum.md");
/// Clodex: dual-model rules (one persistent Builder agent), appended after the voice addendum
/// when the session runs a combo slug.
pub const CLODEX_DUAL_ADDENDUM: &str =
    include_str!("../templates/realtime/clodex_dual_addendum.md");
/// Clodex: tool rules for the voice frontend, appended to the session instructions even when the
/// desktop sends its own prompt.
pub const CLODEX_FRONTEND_ADDENDUM: &str =
    include_str!("../templates/realtime/clodex_frontend_addendum.md");
