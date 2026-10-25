use nightshift_protocol::{SleepPhase, VrchatStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VrchatPresence {
    pub status: VrchatStatus,
    pub status_description: String,
}

impl VrchatPresence {
    pub fn new(status: VrchatStatus, status_description: impl Into<String>) -> Self {
        Self {
            status,
            status_description: status_description.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusAutomationConfig {
    pub enabled: bool,
    pub sleep_status: VrchatStatus,
    pub wake_status: VrchatStatus,
}

impl Default for StatusAutomationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sleep_status: VrchatStatus::AskMe,
            wake_status: VrchatStatus::Active,
        }
    }
}

pub fn sleeping_phase(phase: SleepPhase) -> bool {
    matches!(phase, SleepPhase::EnteringSleep | SleepPhase::Sleeping)
}

pub fn target_for_transition(
    previous_sleeping: bool,
    phase: SleepPhase,
    vrchat_connected: bool,
    current_status: VrchatStatus,
    config: StatusAutomationConfig,
) -> Option<VrchatStatus> {
    if !config.enabled || !vrchat_connected {
        return None;
    }

    let sleeping = sleeping_phase(phase);
    if sleeping == previous_sleeping {
        return None;
    }

    let target = if sleeping {
        config.sleep_status
    } else {
        config.wake_status
    };

    (target != current_status).then_some(target)
}

pub fn target_presence(
    current: &VrchatPresence,
    status: VrchatStatus,
    status_description: &str,
) -> Option<VrchatPresence> {
    let target = VrchatPresence::new(status, status_description);
    (target != *current).then_some(target)
}

pub fn restore_presence(
    captured: Option<&VrchatPresence>,
    current: &VrchatPresence,
) -> Option<VrchatPresence> {
    let captured = captured?;
    (captured != current).then(|| captured.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled() -> StatusAutomationConfig {
        StatusAutomationConfig {
            enabled: true,
            ..StatusAutomationConfig::default()
        }
    }

    #[test]
    fn defaults_off() {
        assert!(!StatusAutomationConfig::default().enabled);
    }

    #[test]
    fn entering_sleep_selects_sleep_status_once() {
        assert_eq!(
            target_for_transition(
                false,
                SleepPhase::EnteringSleep,
                true,
                VrchatStatus::Active,
                enabled(),
            ),
            Some(VrchatStatus::AskMe)
        );
        assert_eq!(
            target_for_transition(
                true,
                SleepPhase::Sleeping,
                true,
                VrchatStatus::Active,
                enabled(),
            ),
            None
        );
    }

    #[test]
    fn waking_selects_wake_status_once() {
        assert_eq!(
            target_for_transition(
                true,
                SleepPhase::Waking,
                true,
                VrchatStatus::AskMe,
                enabled(),
            ),
            Some(VrchatStatus::Active)
        );
        assert_eq!(
            target_for_transition(
                false,
                SleepPhase::Awake,
                true,
                VrchatStatus::AskMe,
                enabled(),
            ),
            None
        );
    }

    #[test]
    fn disabled_or_disconnected_automation_is_inert() {
        assert_eq!(
            target_for_transition(
                false,
                SleepPhase::EnteringSleep,
                true,
                VrchatStatus::Active,
                StatusAutomationConfig::default(),
            ),
            None
        );
        assert_eq!(
            target_for_transition(
                false,
                SleepPhase::EnteringSleep,
                false,
                VrchatStatus::Active,
                enabled(),
            ),
            None
        );
    }

    #[test]
    fn already_matching_status_does_not_generate_a_write() {
        assert_eq!(
            target_for_transition(
                false,
                SleepPhase::EnteringSleep,
                true,
                VrchatStatus::AskMe,
                enabled(),
            ),
            None
        );
    }

    #[test]
    fn offline_can_be_a_deliberate_target_and_can_wake_again() {
        let config = StatusAutomationConfig {
            enabled: true,
            sleep_status: VrchatStatus::Offline,
            wake_status: VrchatStatus::Active,
        };
        assert_eq!(
            target_for_transition(
                false,
                SleepPhase::EnteringSleep,
                true,
                VrchatStatus::Active,
                config,
            ),
            Some(VrchatStatus::Offline)
        );
        assert_eq!(
            target_for_transition(
                true,
                SleepPhase::Waking,
                true,
                VrchatStatus::Offline,
                config,
            ),
            Some(VrchatStatus::Active)
        );
    }

    #[test]
    fn presence_target_compares_status_and_text_together() {
        let current = VrchatPresence::new(VrchatStatus::AskMe, "Still awake");
        assert_eq!(
            target_presence(&current, VrchatStatus::AskMe, "Sleeping in VR 🌙"),
            Some(VrchatPresence::new(
                VrchatStatus::AskMe,
                "Sleeping in VR 🌙"
            ))
        );
        assert_eq!(
            target_presence(&current, VrchatStatus::AskMe, "Still awake"),
            None
        );
    }

    #[test]
    fn restore_returns_captured_presence_only_when_needed() {
        let captured = VrchatPresence::new(VrchatStatus::Busy, "Building pillow fort");
        let sleeping = VrchatPresence::new(VrchatStatus::AskMe, "Sleeping in VR 🌙");
        assert_eq!(
            restore_presence(Some(&captured), &sleeping),
            Some(captured.clone())
        );
        assert_eq!(restore_presence(Some(&captured), &captured), None);
        assert_eq!(restore_presence(None, &sleeping), None);
    }
}
