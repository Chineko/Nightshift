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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_off() {
        assert!(!StatusAutomationConfig::default().enabled);
    }

    #[test]
    fn only_entering_and_sleeping_phases_count_as_sleeping() {
        assert!(sleeping_phase(SleepPhase::EnteringSleep));
        assert!(sleeping_phase(SleepPhase::Sleeping));
        assert!(!sleeping_phase(SleepPhase::Waking));
        assert!(!sleeping_phase(SleepPhase::Awake));
    }
}
