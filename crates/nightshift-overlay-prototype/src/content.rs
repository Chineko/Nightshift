use nightshift_protocol::{AppState, ConnectionState, HrProviderStatus, SleepPhase, VrchatStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelContent {
    pub lines: [String; 6],
    pub sleep_phase: SleepPhase,
    pub brightness_percent: u32,
    pub target_brightness_percent: u32,
    pub brightness_synced: bool,
    pub heart_rate_value: String,
    pub heart_rate_status: String,
    pub osc_status: String,
    pub vrchat_status: String,
    pub automation_summary: String,
    pub connected: bool,
}

impl PanelContent {
    pub fn from_state(state: &AppState) -> Self {
        let brightness_percent = percent(state.brightness.current);
        let target_brightness_percent = percent(state.brightness.sleep_target);
        let lines = legacy_lines(state);
        Self {
            lines,
            sleep_phase: state.sleep.phase,
            brightness_percent,
            target_brightness_percent,
            brightness_synced: !state.brightness.transitioning,
            heart_rate_value: heart_rate_value(state),
            heart_rate_status: heart_rate_status(state),
            osc_status: osc_status(state),
            vrchat_status: vrchat_status(state),
            automation_summary: format!(
                "SLEEP MODE WILL SET BRIGHTNESS TO {target_brightness_percent}% AND UPDATE VRCHAT STATUS."
            ),
            connected: matches!(state.connection, ConnectionState::Connected),
        }
    }

    pub fn sleep_action_is_wake(&self) -> bool {
        matches!(
            self.sleep_phase,
            SleepPhase::EnteringSleep | SleepPhase::Sleeping
        )
    }

    pub fn primary_action_label(&self) -> &'static str {
        if self.sleep_action_is_wake() {
            "WAKE UP"
        } else {
            "ENTER SLEEP MODE"
        }
    }
}

fn legacy_lines(state: &AppState) -> [String; 6] {
    [
        "NIGHTSHIFT".into(),
        legacy_sleep_line(state),
        legacy_brightness_line(state),
        legacy_heart_rate_line(state),
        legacy_vrchat_line(state),
        legacy_automation_line(state),
    ]
}

fn legacy_sleep_line(state: &AppState) -> String {
    let phase = match state.sleep.phase {
        SleepPhase::Awake => "Awake",
        SleepPhase::EnteringSleep => "Entering sleep",
        SleepPhase::Sleeping => "Sleeping",
        SleepPhase::Waking => "Waking",
    };
    if state.sleep.detected_by_hr
        && matches!(
            state.sleep.phase,
            SleepPhase::EnteringSleep | SleepPhase::Sleeping
        )
    {
        format!("Sleep: {phase} (HR)")
    } else {
        format!("Sleep: {phase}")
    }
}

fn legacy_brightness_line(state: &AppState) -> String {
    let current = percent(state.brightness.current);
    if state.brightness.transitioning {
        format!(
            "Brightness: {current}% -> {}%",
            percent(state.brightness.target)
        )
    } else {
        format!("Brightness: {current}%")
    }
}

fn legacy_heart_rate_line(state: &AppState) -> String {
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

fn legacy_vrchat_line(state: &AppState) -> String {
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

fn legacy_automation_line(state: &AppState) -> String {
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
        use nightshift_protocol::InviteProfile;
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

fn heart_rate_value(state: &AppState) -> String {
    if !state.heart_rate.enabled {
        return "OFF".into();
    }
    state
        .heart_rate
        .bpm
        .map(|bpm| format!("{bpm} BPM"))
        .unwrap_or_else(|| "-- BPM".into())
}

fn heart_rate_status(state: &AppState) -> String {
    if !state.heart_rate.enabled {
        return "DISABLED".into();
    }
    if state.heart_rate.bpm.is_some() {
        return "NORMAL".into();
    }
    match state.heart_rate.provider_status {
        HrProviderStatus::Disabled => "DISABLED",
        HrProviderStatus::Searching => "SEARCHING",
        HrProviderStatus::Connected => "CONNECTED",
        HrProviderStatus::Error => "ERROR",
        HrProviderStatus::Unknown => "WAITING",
    }
    .into()
}

fn osc_status(state: &AppState) -> String {
    if !state.osc.enabled {
        "OFF"
    } else if state.osc.connected {
        "RUNNING"
    } else {
        "WAITING"
    }
    .into()
}

fn vrchat_status(state: &AppState) -> String {
    if !state.vrchat.connected {
        return "OFFLINE".into();
    }
    match state.vrchat.status {
        VrchatStatus::Active => "ACTIVE",
        VrchatStatus::JoinMe => "JOIN ME",
        VrchatStatus::AskMe => "ASK ME",
        VrchatStatus::Busy => "BUSY",
        VrchatStatus::Offline => "OFFLINE",
    }
    .into()
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
    fn default_state_builds_home_dashboard_content() {
        let content = PanelContent::from_state(&AppState::default());
        assert_eq!(content.brightness_percent, 100);
        assert_eq!(content.heart_rate_value, "OFF");
        assert_eq!(content.osc_status, "OFF");
        assert_eq!(content.vrchat_status, "OFFLINE");
        assert_eq!(content.primary_action_label(), "ENTER SLEEP MODE");
    }

    #[test]
    fn sleeping_state_switches_primary_action_to_wake() {
        let mut state = AppState::default();
        state.sleep.phase = SleepPhase::Sleeping;
        assert!(PanelContent::from_state(&state).sleep_action_is_wake());
        assert_eq!(
            PanelContent::from_state(&state).primary_action_label(),
            "WAKE UP"
        );
    }

    #[test]
    fn non_finite_brightness_fails_to_safe_readable_value() {
        let mut state = AppState::default();
        state.brightness.current = f32::NAN;
        assert_eq!(PanelContent::from_state(&state).brightness_percent, 100);
    }
}
