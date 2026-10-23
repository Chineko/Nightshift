use nightshift_protocol::{SleepPhase, VrchatStatus};

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
}
