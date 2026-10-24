use nightshift_protocol::{
    AppState, HrProviderStatus, InviteProfile, SleepPhase, VrchatStatus,
};

pub const PANEL_LINE_COUNT: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelContent {
    pub lines: [String; PANEL_LINE_COUNT],
}

impl PanelContent {
    pub fn from_state(state: &AppState) -> Self {
        Self {
            lines: [
                "NIGHTSHIFT".into(),
                sleep_line(state),
                brightness_line(state),
                heart_rate_line(state),
                vrchat_line(state),
                automation_line(state),
            ],
        }
    }
}

fn sleep_line(state: &AppState) -> String {
    let phase = match state.sleep.phase {
        SleepPhase::Awake => "Awake",
        SleepPhase::EnteringSleep => "Entering sleep",
        SleepPhase::Sleeping => "Sleeping",
        SleepPhase::Waking => "Waking",
    };
    if state.sleep.detected_by_hr && matches!(state.sleep.phase, SleepPhase::EnteringSleep | SleepPhase::Sleeping) {
        format!("Sleep: {phase} (HR)")
    } else {
        format!("Sleep: {phase}")
    }
}

fn brightness_line(state: &AppState) -> String {
    let current = percent(state.brightness.current);
    if state.brightness.transitioning {
        format!("Brightness: {current}% -> {}%", percent(state.brightness.target))
    } else {
        format!("Brightness: {current}%")
    }
}

fn heart_rate_line(state: &AppState) -> String {
    if !state.heart_rate.enabled {
        return "Heart rate: Off".into();
    }
    if let Some(bpm) = state.heart_rate.bpm {
        return format!("Heart rate: {bpm} BPM");
    }
    let status = match state.heart_rate.provider_status {
        HrProviderStatus::Disabled => "Disabled",
        HrProviderStatus::Searching => "Searching",
        HrProviderStatus::Connected => "Connected",
        HrProviderStatus::Error => "Error",
        HrProviderStatus::Unknown => "Waiting",
    };
    format!("Heart rate: {status}")
}

fn vrchat_line(state: &AppState) -> String {
    if !state.vrchat.connected {
        return "VRChat: Offline".into();
    }
    let status = match state.vrchat.status {
        VrchatStatus::Active => "Active",
        VrchatStatus::JoinMe => "Join Me",
        VrchatStatus::AskMe => "Ask Me",
        VrchatStatus::Busy => "Busy",
        VrchatStatus::Offline => "Offline",
    };
    format!("VRChat: {status}")
}

fn automation_line(state: &AppState) -> String {
    let osc = if !state.osc.enabled {
        "OSC off"
    } else if state.osc.connected {
        "OSC connected"
    } else {
        "OSC waiting"
    };
    let invites = if !state.invites.enabled {
        "Invites off"
    } else {
        match state.invites.profile {
            InviteProfile::Disabled => "Invites disabled",
            InviteProfile::Friends => "Invites friends",
            InviteProfile::Favorites => "Invites favorites",
            InviteProfile::Whitelist => "Invites whitelist",
            InviteProfile::Custom => "Invites custom",
        }
    };
    format!("{osc} | {invites}")
}

fn percent(value: f32) -> u32 {
    if value.is_finite() {
        (value.clamp(0.0, 1.0) * 100.0).round() as u32
    } else {
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_produces_conservative_status_lines() {
        let content = PanelContent::from_state(&AppState::default());
        assert_eq!(content.lines[0], "NIGHTSHIFT");
        assert_eq!(content.lines[1], "Sleep: Awake");
        assert_eq!(content.lines[2], "Brightness: 100%");
        assert_eq!(content.lines[3], "Heart rate: Off");
        assert_eq!(content.lines[4], "VRChat: Offline");
        assert_eq!(content.lines[5], "OSC off | Invites off");
    }

    #[test]
    fn active_state_surfaces_sleep_hr_and_automation_context() {
        let mut state = AppState::default();
        state.sleep.phase = SleepPhase::Sleeping;
        state.sleep.detected_by_hr = true;
        state.brightness.current = 0.42;
        state.brightness.target = 0.06;
        state.brightness.transitioning = true;
        state.heart_rate.enabled = true;
        state.heart_rate.provider_status = HrProviderStatus::Connected;
        state.heart_rate.bpm = Some(54);
        state.vrchat.connected = true;
        state.vrchat.status = VrchatStatus::AskMe;
        state.osc.enabled = true;
        state.osc.connected = true;
        state.invites.enabled = true;
        state.invites.profile = InviteProfile::Friends;

        let content = PanelContent::from_state(&state);
        assert_eq!(content.lines[1], "Sleep: Sleeping (HR)");
        assert_eq!(content.lines[2], "Brightness: 42% -> 6%");
        assert_eq!(content.lines[3], "Heart rate: 54 BPM");
        assert_eq!(content.lines[4], "VRChat: Ask Me");
        assert_eq!(content.lines[5], "OSC connected | Invites friends");
    }

    #[test]
    fn non_finite_brightness_fails_to_safe_readable_value() {
        let mut state = AppState::default();
        state.brightness.current = f32::NAN;
        assert_eq!(PanelContent::from_state(&state).lines[2], "Brightness: 100%");
    }
}
