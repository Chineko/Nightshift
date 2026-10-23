#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalmPeriodConfig {
    pub heart_rate_threshold_bpm: u16,
    pub calm_period_ms: u64,
    pub stale_sample_ms: u64,
    pub manual_wake_cooldown_ms: u64,
    pub retrigger_suppression_ms: u64,
}

impl Default for CalmPeriodConfig {
    fn default() -> Self {
        Self {
            heart_rate_threshold_bpm: 60,
            calm_period_ms: 120_000,
            stale_sample_ms: 20_000,
            manual_wake_cooldown_ms: 15 * 60_000,
            retrigger_suppression_ms: 5 * 60_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectionState {
    AboveThreshold,
    StaleReset,
    Calming { remaining_ms: u64 },
    ManualWakeCooldown { remaining_ms: u64 },
    RetriggerSuppressed { remaining_ms: u64 },
    AlreadySleeping,
    SleepCandidate,
}

pub struct CalmPeriodDetector {
    config: CalmPeriodConfig,
    threshold_last_exceeded_ms: Option<u64>,
    heart_rate_last_received_ms: Option<u64>,
    sleep_last_disabled_ms: Option<u64>,
    suppress_until_ms: u64,
}

impl CalmPeriodDetector {
    pub fn new(config: CalmPeriodConfig) -> Self {
        Self {
            config,
            threshold_last_exceeded_ms: None,
            heart_rate_last_received_ms: None,
            sleep_last_disabled_ms: None,
            suppress_until_ms: 0,
        }
    }

    pub fn note_sleep_disabled(&mut self, now_ms: u64) {
        self.sleep_last_disabled_ms = Some(now_ms);
    }

    pub fn observe(&mut self, now_ms: u64, bpm: u16, sleep_mode: bool) -> DetectionState {
        let stale = self
            .heart_rate_last_received_ms
            .is_none_or(|last| now_ms.saturating_sub(last) > self.config.stale_sample_ms);
        self.heart_rate_last_received_ms = Some(now_ms);

        if bpm >= self.config.heart_rate_threshold_bpm {
            self.threshold_last_exceeded_ms = Some(now_ms);
            return DetectionState::AboveThreshold;
        }

        if stale {
            self.threshold_last_exceeded_ms = Some(now_ms);
            return DetectionState::StaleReset;
        }

        let calm_started_ms = self.threshold_last_exceeded_ms.get_or_insert(now_ms);
        let calm_elapsed_ms = now_ms.saturating_sub(*calm_started_ms);
        if calm_elapsed_ms < self.config.calm_period_ms {
            return DetectionState::Calming {
                remaining_ms: self.config.calm_period_ms - calm_elapsed_ms,
            };
        }

        if sleep_mode {
            return DetectionState::AlreadySleeping;
        }

        if let Some(disabled_ms) = self.sleep_last_disabled_ms {
            let elapsed = now_ms.saturating_sub(disabled_ms);
            if elapsed <= self.config.manual_wake_cooldown_ms {
                return DetectionState::ManualWakeCooldown {
                    remaining_ms: self.config.manual_wake_cooldown_ms.saturating_sub(elapsed),
                };
            }
        }

        if now_ms < self.suppress_until_ms {
            return DetectionState::RetriggerSuppressed {
                remaining_ms: self.suppress_until_ms - now_ms,
            };
        }

        self.suppress_until_ms = now_ms.saturating_add(self.config.retrigger_suppression_ms);
        DetectionState::SleepCandidate
    }
}

impl Default for CalmPeriodDetector {
    fn default() -> Self {
        Self::new(CalmPeriodConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detector() -> CalmPeriodDetector {
        CalmPeriodDetector::default()
    }

    #[test]
    fn defaults_match_oyasumi_baseline() {
        let config = CalmPeriodConfig::default();
        assert_eq!(config.heart_rate_threshold_bpm, 60);
        assert_eq!(config.calm_period_ms, 120_000);
        assert_eq!(config.stale_sample_ms, 20_000);
        assert_eq!(config.manual_wake_cooldown_ms, 15 * 60_000);
        assert_eq!(config.retrigger_suppression_ms, 5 * 60_000);
    }

    #[test]
    fn first_sample_starts_a_fresh_calm_period() {
        let mut detector = detector();
        assert_eq!(detector.observe(1_000, 55, false), DetectionState::StaleReset);
        assert_eq!(
            detector.observe(2_000, 55, false),
            DetectionState::Calming { remaining_ms: 119_000 }
        );
    }

    #[test]
    fn above_threshold_resets_calm_period() {
        let mut detector = detector();
        detector.observe(1_000, 55, false);
        detector.observe(61_000, 55, false);
        assert_eq!(detector.observe(62_000, 70, false), DetectionState::AboveThreshold);
        assert_eq!(
            detector.observe(63_000, 55, false),
            DetectionState::Calming { remaining_ms: 119_000 }
        );
    }

    #[test]
    fn stale_gap_resets_calm_period() {
        let mut detector = detector();
        detector.observe(1_000, 55, false);
        assert_eq!(detector.observe(25_000, 55, false), DetectionState::StaleReset);
    }

    #[test]
    fn calm_period_produces_sleep_candidate() {
        let mut detector = detector();
        detector.observe(1_000, 55, false);
        assert_eq!(detector.observe(121_000, 55, false), DetectionState::StaleReset);
        // Keep samples fresh during the actual calm window.
        let mut detector = detector();
        detector.observe(0, 55, false);
        for now in (10_000..=120_000).step_by(10_000) {
            let state = detector.observe(now, 55, false);
            if now < 120_000 {
                assert!(matches!(state, DetectionState::Calming { .. }));
            } else {
                assert_eq!(state, DetectionState::SleepCandidate);
            }
        }
    }

    #[test]
    fn manual_wake_blocks_immediate_reentry() {
        let mut detector = detector();
        detector.note_sleep_disabled(0);
        detector.observe(0, 55, false);
        for now in (10_000..=120_000).step_by(10_000) {
            let state = detector.observe(now, 55, false);
            if now == 120_000 {
                assert!(matches!(state, DetectionState::ManualWakeCooldown { .. }));
            }
        }
    }

    #[test]
    fn detector_does_not_trigger_when_already_sleeping() {
        let mut detector = detector();
        detector.observe(0, 55, true);
        for now in (10_000..=120_000).step_by(10_000) {
            let state = detector.observe(now, 55, true);
            if now == 120_000 {
                assert_eq!(state, DetectionState::AlreadySleeping);
            }
        }
    }

    #[test]
    fn candidate_sets_retrigger_suppression() {
        let mut detector = detector();
        detector.observe(0, 55, false);
        for now in (10_000..=120_000).step_by(10_000) {
            detector.observe(now, 55, false);
        }
        assert!(matches!(
            detector.observe(130_000, 55, false),
            DetectionState::RetriggerSuppressed { .. }
        ));
    }
}
