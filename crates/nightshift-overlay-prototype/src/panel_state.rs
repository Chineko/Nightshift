use nightshift_protocol::{AppState, StateEvent};

use crate::content::PanelContent;

#[derive(Clone, Debug)]
pub struct PanelStateTracker {
    state: AppState,
    content: PanelContent,
}

impl Default for PanelStateTracker {
    fn default() -> Self {
        Self::from_state(AppState::default())
    }
}

impl PanelStateTracker {
    pub fn from_state(state: AppState) -> Self {
        let content = PanelContent::from_state(&state);
        Self { state, content }
    }

    pub fn replace_state(&mut self, state: AppState) -> Option<PanelContent> {
        self.state = state;
        self.refresh_content()
    }

    pub fn apply_event(&mut self, event: &StateEvent) -> Option<PanelContent> {
        match event {
            StateEvent::SleepChanged(value) => self.state.sleep = value.clone(),
            StateEvent::BrightnessChanged(value) => self.state.brightness = value.clone(),
            StateEvent::HeartRateChanged(value) => self.state.heart_rate = value.clone(),
            StateEvent::OscChanged(value) => self.state.osc = value.clone(),
            StateEvent::VrchatChanged(value) => self.state.vrchat = value.clone(),
            StateEvent::InvitesChanged(value) => self.state.invites = value.clone(),
            StateEvent::OverlayChanged(value) => self.state.overlay = value.clone(),
            StateEvent::SettingsChanged(value) => self.state.settings = value.clone(),
            StateEvent::ConnectionChanged(value) => self.state.connection = *value,
        }
        self.refresh_content()
    }

    pub fn content(&self) -> &PanelContent {
        &self.content
    }

    fn refresh_content(&mut self) -> Option<PanelContent> {
        let next = PanelContent::from_state(&self.state);
        if next == self.content {
            return None;
        }
        self.content = next.clone();
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use nightshift_protocol::{BrightnessState, ConnectionState, SleepPhase, SleepState};

    use super::*;

    #[test]
    fn visible_state_change_emits_new_content() {
        let mut tracker = PanelStateTracker::default();
        let changed = tracker.apply_event(&StateEvent::SleepChanged(SleepState {
            phase: SleepPhase::Sleeping,
            ..SleepState::default()
        }));
        assert_eq!(changed.unwrap().lines[1], "Sleep: Sleeping");
    }

    #[test]
    fn invisible_state_change_does_not_request_redraw() {
        let mut tracker = PanelStateTracker::default();
        assert!(tracker
            .apply_event(&StateEvent::ConnectionChanged(ConnectionState::Degraded))
            .is_none());
    }

    #[test]
    fn equivalent_visible_content_is_deduplicated() {
        let mut tracker = PanelStateTracker::default();
        let brightness = BrightnessState::default();
        assert!(tracker
            .apply_event(&StateEvent::BrightnessChanged(brightness))
            .is_none());
    }

    #[test]
    fn full_state_replacement_emits_only_when_panel_text_changes() {
        let mut tracker = PanelStateTracker::default();
        assert!(tracker.replace_state(AppState::default()).is_none());

        let mut state = AppState::default();
        state.sleep.phase = SleepPhase::Waking;
        assert_eq!(
            tracker.replace_state(state).unwrap().lines[1],
            "Sleep: Waking"
        );
    }
}
