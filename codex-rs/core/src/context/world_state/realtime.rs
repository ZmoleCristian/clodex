use super::PreviousSectionState;
use super::SectionTransition;
use super::WorldStateSection;
use super::WorldStateUpdate;
use crate::context::ContextualUserFragment;
use crate::context::RealtimeEndInstructions;
use crate::context::RealtimeStartInstructions;
use crate::context::RealtimeStartWithInstructions;
use serde::Deserialize;
use serde::Serialize;

/// The realtime conversation state currently visible to the model.
#[derive(Clone, Debug)]
pub(crate) struct RealtimeState {
    snapshot: RealtimeSnapshot,
    start_instructions: Option<String>,
    end_instructions: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct RealtimeSnapshot {
    active: bool,
}

impl RealtimeState {
    pub(crate) fn new(
        active: bool,
        dual: bool,
        start_instructions: Option<&str>,
        end_instructions: Option<&str>,
    ) -> Self {
        // Clodex: whatever the desktop sends as start instructions (or the upstream
        // default), the voice addendum always follows it; a combo slug adds the dual addendum.
        let base = start_instructions.unwrap_or(codex_prompts::START_INSTRUCTIONS);
        let mut start_instructions = format!(
            "{}\n\n{}",
            base.trim(),
            codex_prompts::CLODEX_VOICE_ADDENDUM.trim()
        );
        if dual {
            start_instructions.push_str("\n\n");
            start_instructions.push_str(codex_prompts::CLODEX_DUAL_ADDENDUM.trim());
        }
        Self {
            snapshot: RealtimeSnapshot { active },
            start_instructions: Some(start_instructions),
            end_instructions: end_instructions.map(str::to_string),
        }
    }

    fn render_start(&self) -> Box<dyn ContextualUserFragment> {
        match self.start_instructions.as_deref() {
            Some(instructions) => Box::new(RealtimeStartWithInstructions::new(instructions)),
            None => Box::new(RealtimeStartInstructions),
        }
    }

    fn render_transition(&self, previous_active: bool) -> Option<Box<dyn ContextualUserFragment>> {
        match (previous_active, self.snapshot.active) {
            (false, true) => Some(self.render_start()),
            (true, false) => Some(match self.end_instructions.as_deref() {
                Some(instructions) => {
                    Box::new(RealtimeEndInstructions::with_instructions(instructions))
                }
                None => Box::new(RealtimeEndInstructions::new()),
            }),
            (false, false) | (true, true) => None,
        }
    }
}

impl WorldStateSection for RealtimeState {
    const ID: &'static str = "realtime";
    type Snapshot = RealtimeSnapshot;

    fn matches_legacy_fragment(role: &str, text: &str) -> bool {
        role == "developer" && RealtimeStartInstructions::matches_text(text)
    }

    fn has_retained_fragment_matcher() -> bool {
        true
    }

    fn matches_retained_fragment(role: &str, text: &str) -> bool {
        Self::matches_legacy_fragment(role, text)
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> SectionTransition<Self::Snapshot> {
        let fragment = match previous {
            PreviousSectionState::Known(previous) if previous == &self.snapshot => {
                return (None, Vec::new());
            }
            PreviousSectionState::Known(previous) => self.render_transition(previous.active),
            PreviousSectionState::Absent | PreviousSectionState::Unknown
                if self.snapshot.active =>
            {
                Some(self.render_start())
            }
            PreviousSectionState::Absent | PreviousSectionState::Unknown => None,
        };
        (
            Some(self.snapshot.clone()),
            WorldStateUpdate::optional_boxed_fragment(fragment),
        )
    }
}

#[cfg(test)]
#[path = "realtime_tests.rs"]
mod tests;
