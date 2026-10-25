use super::*;

pub(crate) fn detector_config_from_state(state: &AppState) -> CalmPeriodConfig {
    CalmPeriodConfig {
        heart_rate_threshold_bpm: state.heart_rate.threshold_bpm,
        calm_period_ms: state.heart_rate.calm_period_seconds.saturating_mul(1_000),
        manual_wake_cooldown_ms: state.heart_rate.cooldown_seconds.saturating_mul(1_000),
        ..CalmPeriodConfig::default()
    }
}

pub(crate) fn normalize_hr_devices(devices: Vec<HrDeviceInfo>) -> Vec<HrDeviceInfo> {
    let mut normalized = Vec::with_capacity(devices.len().min(MAX_HR_DISCOVERED_DEVICES));
    for mut device in devices {
        device.id = device.id.trim().to_string();
        device.address = device.address.trim().to_string();
        device.name = device
            .name
            .take()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty());
        if device.id.is_empty()
            || normalized
                .iter()
                .any(|known: &HrDeviceInfo| known.id == device.id)
        {
            continue;
        }
        normalized.push(device);
        if normalized.len() == MAX_HR_DISCOVERED_DEVICES {
            break;
        }
    }
    normalized
}

#[derive(Clone)]
pub(crate) struct StateStore {
    pub(crate) state: Arc<Mutex<AppState>>,
    pub(crate) updates: broadcast::Sender<StateEvent>,
    pub(crate) overlay_clients: Arc<AtomicUsize>,
    pub(crate) osc_clients: Arc<AtomicUsize>,
    pub(crate) hr_clients: Arc<AtomicUsize>,
    pub(crate) vrchat_clients: Arc<AtomicUsize>,
    pub(crate) transition_generation: Arc<AtomicU64>,
    pub(crate) config_updates: watch::Sender<PersistedConfig>,
    pub(crate) config_save_lock: Arc<Mutex<()>>,
    pub(crate) config_writer_shutdown: Arc<AtomicBool>,
    pub(crate) hr_detector: Arc<Mutex<CalmPeriodDetector>>,
    pub(crate) started: Instant,
}

impl StateStore {
    pub(crate) fn new(initial_state: AppState) -> Self {
        let (updates, _) = broadcast::channel(64);
        let initial_config = PersistedConfig::from_state(&initial_state);
        let (config_updates, mut config_rx) = watch::channel(initial_config);
        let config_save_lock = Arc::new(Mutex::new(()));
        let config_writer_shutdown = Arc::new(AtomicBool::new(false));
        let writer_save_lock = config_save_lock.clone();
        let writer_shutdown = config_writer_shutdown.clone();
        let hr_detector = CalmPeriodDetector::new(detector_config_from_state(&initial_state));

        tokio::spawn(async move {
            while config_rx.changed().await.is_ok() {
                tokio::time::sleep(CONFIG_WRITE_DEBOUNCE).await;
                let config = config_rx.borrow_and_update().clone();
                let _save_guard = writer_save_lock.lock().await;
                if writer_shutdown.load(Ordering::Acquire) {
                    break;
                }
                if let Err(error) = config.save().await {
                    warn!(%error, "failed to persist Nightshift settings");
                }
            }
        });

        Self {
            state: Arc::new(Mutex::new(initial_state)),
            updates,
            overlay_clients: Arc::new(AtomicUsize::new(0)),
            osc_clients: Arc::new(AtomicUsize::new(0)),
            hr_clients: Arc::new(AtomicUsize::new(0)),
            vrchat_clients: Arc::new(AtomicUsize::new(0)),
            transition_generation: Arc::new(AtomicU64::new(0)),
            config_updates,
            config_save_lock,
            config_writer_shutdown,
            hr_detector: Arc::new(Mutex::new(hr_detector)),
            started: Instant::now(),
        }
    }

    pub(crate) fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub(crate) async fn snapshot_and_subscribe(&self) -> (AppState, broadcast::Receiver<StateEvent>) {
        let state = self.state.lock().await;
        let updates = self.updates.subscribe();
        (state.clone(), updates)
    }

    pub(crate) async fn flush_config(&self) -> Result<()> {
        self.config_writer_shutdown.store(true, Ordering::Release);
        let config = {
            let state = self.state.lock().await;
            PersistedConfig::from_state(&state)
        };
        let _save_guard = self.config_save_lock.lock().await;
        config.save().await
    }

    pub(crate) fn emit_changes(&self, before: &AppState, after: &AppState) {
        let emit = |event| {
            let _ = self.updates.send(event);
        };

        if before.connection != after.connection {
            emit(StateEvent::ConnectionChanged(after.connection));
        }
        if before.sleep != after.sleep {
            emit(StateEvent::SleepChanged(after.sleep.clone()));
        }
        if before.brightness != after.brightness {
            emit(StateEvent::BrightnessChanged(after.brightness.clone()));
        }
        if before.heart_rate != after.heart_rate {
            emit(StateEvent::HeartRateChanged(after.heart_rate.clone()));
        }
        if before.osc != after.osc {
            emit(StateEvent::OscChanged(after.osc.clone()));
        }
        if before.vrchat != after.vrchat {
            emit(StateEvent::VrchatChanged(after.vrchat.clone()));
        }
        if before.invites != after.invites {
            emit(StateEvent::InvitesChanged(after.invites.clone()));
        }
        if before.overlay != after.overlay {
            emit(StateEvent::OverlayChanged(after.overlay.clone()));
        }
        if before.settings != after.settings {
            emit(StateEvent::SettingsChanged(after.settings.clone()));
        }
    }

    pub(crate) async fn set_overlay_runtime(&self, connected: bool, client_version: Option<String>) {
        let mut state = self.state.lock().await;
        let before = state.clone();
        state.overlay.runtime_connected = connected;
        state.overlay.runtime_client_version = if connected { client_version } else { None };

        if *state != before {
            self.emit_changes(&before, &state);
        }
    }

    pub(crate) async fn set_overlay_remembered_pose(&self, pose: OverlayPose) {
        let Some(pose) = sanitize_overlay_pose(pose) else {
            warn!("ignoring invalid overlay remembered-pose report");
            return;
        };

        let mut state = self.state.lock().await;
        if self.config_writer_shutdown.load(Ordering::Acquire) {
            return;
        }
        if state.overlay.remembered_pose == Some(pose) {
            return;
        }
        let before = state.clone();
        state.overlay.remembered_pose = Some(pose);
        self.emit_changes(&before, &state);
        self.config_updates
            .send_replace(PersistedConfig::from_state(&state));
    }

    pub(crate) async fn set_sleeping_pose(&self, pose: SleepingPose) {
        let mut state = self.state.lock().await;
        let before = state.clone();
        state.sleep.pose = pose;

        if *state != before {
            self.emit_changes(&before, &state);
        }
    }

    pub(crate) async fn set_osc_runtime(
        &self,
        connected: bool,
        avatar_id: Option<String>,
        avatar_parameter_count: u32,
        gogo_detected: bool,
    ) {
        let mut state = self.state.lock().await;
        let before = state.clone();
        state.osc.connected = connected;
        state.osc.avatar_id = if connected { avatar_id } else { None };
        state.osc.avatar_parameter_count = if connected { avatar_parameter_count } else { 0 };
        state.osc.gogo_detected = connected && gogo_detected;

        if *state != before {
            self.emit_changes(&before, &state);
        }
    }

    pub(crate) async fn set_vrchat_runtime(
        &self,
        connected: bool,
        display_name: Option<String>,
        status: VrchatStatus,
        last_invite_action: Option<InviteAction>,
        invite_error: Option<String>,
    ) {
        let mut state = self.state.lock().await;
        let before = state.clone();
        state.vrchat.connected = connected;
        state.vrchat.display_name = if connected { display_name } else { None };
        state.vrchat.status = if connected { status } else { VrchatStatus::Offline };

        if let Some(action) = last_invite_action {
            state.invites.last_action = Some(action);
            state.invites.last_error = None;
        }
        if let Some(error) = invite_error {
            state.invites.last_error = Some(error);
        }

        if *state != before {
            self.emit_changes(&before, &state);
        }
    }

    pub(crate) async fn set_hr_devices(&self, devices: Vec<HrDeviceInfo>) {
        let mut state = self.state.lock().await;
        let before = state.clone();
        state.heart_rate.available_devices = normalize_hr_devices(devices);
        if *state != before {
            self.emit_changes(&before, &state);
        }
    }

    pub(crate) async fn set_hr_provider_runtime(
        &self,
        status: HrProviderStatus,
        connected_device: Option<String>,
        last_error: Option<String>,
    ) {
        let mut state = self.state.lock().await;
        let before = state.clone();

        state.heart_rate.provider_status = status;
        state.heart_rate.connected_device = if status == HrProviderStatus::Connected {
            connected_device
        } else {
            None
        };

        match status {
            HrProviderStatus::Error => state.heart_rate.last_error = last_error,
            HrProviderStatus::Connected | HrProviderStatus::Disabled => {
                state.heart_rate.last_error = None;
            }
            HrProviderStatus::Searching | HrProviderStatus::Unknown => {}
        }

        if matches!(
            status,
            HrProviderStatus::Searching | HrProviderStatus::Disabled | HrProviderStatus::Unknown
        ) {
            state.heart_rate.available_devices.clear();
        }

        if status != HrProviderStatus::Connected {
            state.heart_rate.connected = false;
            state.heart_rate.bpm = None;
            state.heart_rate.status = HrStatus::Unknown;
        }

        if *state != before {
            self.emit_changes(&before, &state);
        }
    }

    pub(crate) async fn set_hr_runtime(&self, connected: bool, bpm: Option<u16>) {
        if !connected {
            let mut state = self.state.lock().await;
            let before = state.clone();
            state.heart_rate.connected = false;
            state.heart_rate.bpm = None;
            state.heart_rate.status = HrStatus::Unknown;
            if *state != before {
                self.emit_changes(&before, &state);
            }
            return;
        }

        let Some(bpm) = bpm else {
            let mut state = self.state.lock().await;
            let before = state.clone();
            state.heart_rate.connected = true;
            state.heart_rate.bpm = None;
            state.heart_rate.status = HrStatus::Unknown;
            if *state != before {
                self.emit_changes(&before, &state);
            }
            return;
        };

        let (sleeping, monitoring_enabled, detection_enabled) = {
            let state = self.state.lock().await;
            (
                matches!(
                    state.sleep.phase,
                    SleepPhase::EnteringSleep | SleepPhase::Sleeping
                ),
                state.heart_rate.enabled,
                state.sleep.detection_enabled,
            )
        };

        let detection = self
            .hr_detector
            .lock()
            .await
            .observe(self.now_ms(), bpm, sleeping);
        let status = match detection {
            DetectionState::AboveThreshold => HrStatus::Normal,
            DetectionState::StaleReset => HrStatus::Unknown,
            DetectionState::Calming { .. }
            | DetectionState::ManualWakeCooldown { .. }
            | DetectionState::RetriggerSuppressed { .. }
            | DetectionState::AlreadySleeping => HrStatus::Resting,
            DetectionState::SleepCandidate => HrStatus::SleepCandidate,
        };

        {
            let mut state = self.state.lock().await;
            let before = state.clone();
            state.heart_rate.connected = true;
            state.heart_rate.bpm = Some(bpm);
            state.heart_rate.status = status;
            if *state != before {
                self.emit_changes(&before, &state);
            }
        }

        if detection == DetectionState::SleepCandidate
            && monitoring_enabled
            && detection_enabled
            && !sleeping
        {
            self.apply(UiCommand::EnterSleep).await;
            let mut state = self.state.lock().await;
            let before = state.clone();
            state.sleep.detected_by_hr = true;
            if *state != before {
                self.emit_changes(&before, &state);
            }
        }
    }

    pub(crate) async fn apply_worker_report(&self, client: &str, report: WorkerReport) -> bool {
        match report {
            WorkerReport::OscRuntime {
                connected,
                avatar_id,
                avatar_parameter_count,
                gogo_detected,
            } if client == "nightshift-osc" => {
                self.set_osc_runtime(
                    connected,
                    avatar_id,
                    avatar_parameter_count,
                    gogo_detected,
                )
                .await;
                true
            }
            WorkerReport::SleepingPose { pose }
                if matches!(
                    client,
                    "nightshift-overlay" | "nightshift-overlay-prototype"
                ) =>
            {
                self.set_sleeping_pose(pose).await;
                true
            }
            WorkerReport::OverlayRuntimePose { pose }
                if matches!(
                    client,
                    "nightshift-overlay" | "nightshift-overlay-prototype"
                ) =>
            {
                self.set_overlay_remembered_pose(pose).await;
                true
            }
            WorkerReport::HeartRateRuntime { connected, bpm } if client == "nightshift-hr" => {
                self.set_hr_runtime(connected, bpm).await;
                true
            }
            WorkerReport::HeartRateProviderRuntime {
                status,
                connected_device,
                last_error,
            } if client == "nightshift-hr" => {
                self.set_hr_provider_runtime(status, connected_device, last_error)
                    .await;
                true
            }
            WorkerReport::HeartRateDevices { devices } if client == "nightshift-hr" => {
                self.set_hr_devices(devices).await;
                true
            }
            WorkerReport::VrchatRuntime {
                connected,
                display_name,
                status,
                last_invite_action,
                invite_error,
            } if client == "nightshift-vrchat" => {
                self.set_vrchat_runtime(
                    connected,
                    display_name,
                    status,
                    last_invite_action,
                    invite_error,
                )
                .await;
                true
            }
            _ => false,
        }
    }

    pub(crate) async fn complete_transition(
        &self,
        transition_id: u64,
        expected_phase: SleepPhase,
        final_phase: SleepPhase,
        final_brightness: f32,
    ) {
        let mut state = self.state.lock().await;
        if self.transition_generation.load(Ordering::Acquire) != transition_id
            || state.sleep.phase != expected_phase
        {
            return;
        }

        let before = state.clone();
        state.sleep.phase = final_phase;

        if !state.brightness.manual_override {
            state.brightness.current = final_brightness;
            state.brightness.target = final_brightness;
        }
        state.brightness.transitioning = false;

        self.emit_changes(&before, &state);
    }

    pub(crate) async fn apply(&self, command: UiCommand) {
        let persist = matches!(
            &command,
            UiCommand::SetSleepBrightness(_)
                | UiCommand::SetFadeDuration(_)
                | UiCommand::SetHrMonitoring(_)
                | UiCommand::SetHrDeviceFilter(_)
                | UiCommand::SetHrThreshold(_)
                | UiCommand::SetHrCalmPeriod(_)
                | UiCommand::SetHrCooldown(_)
                | UiCommand::SetSleepDetection(_)
                | UiCommand::SetOscAutomation(_)
                | UiCommand::SetGogoAutomation(_)
                | UiCommand::SetVrchatStatusAutomation(_)
                | UiCommand::SetVrchatSleepStatus(_)
                | UiCommand::SetVrchatWakeStatus(_)
                | UiCommand::SetInviteAutomation(_)
                | UiCommand::SetInviteProfile(_)
                | UiCommand::SetInviteRules(_)
                | UiCommand::SetOverlaySpawnDistance(_)
                | UiCommand::SetOverlaySpawnMode(_)
                | UiCommand::SetOverlayLaserEnabled(_)
        );

        let mut state = self.state.lock().await;
        if self.config_writer_shutdown.load(Ordering::Acquire) {
            return;
        }
        let before = state.clone();
        let mut transition = None;
        let mut manual_wake = false;
        let mut hr_detector_config = None;

        match command {
            UiCommand::EnterSleep => {
                if !matches!(state.sleep.phase, SleepPhase::EnteringSleep | SleepPhase::Sleeping) {
                    let transition_id = self.transition_generation.fetch_add(1, Ordering::AcqRel) + 1;
                    state.sleep.detected_by_hr = false;
                    state.sleep.phase = SleepPhase::EnteringSleep;
                    state.brightness.target = state.brightness.sleep_target;
                    state.brightness.transitioning = true;
                    state.brightness.manual_override = false;
                    transition = Some((
                        transition_id,
                        SleepPhase::EnteringSleep,
                        SleepPhase::Sleeping,
                        state.brightness.sleep_target,
                        state.settings.fade_duration_seconds,
                    ));
                }
            }
            UiCommand::Wake => {
                if !matches!(state.sleep.phase, SleepPhase::Waking | SleepPhase::Awake) {
                    let transition_id = self.transition_generation.fetch_add(1, Ordering::AcqRel) + 1;
                    state.sleep.detected_by_hr = false;
                    state.sleep.phase = SleepPhase::Waking;
                    state.brightness.target = 1.0;
                    state.brightness.transitioning = true;
                    state.brightness.manual_override = false;
                    manual_wake = true;
                    transition = Some((
                        transition_id,
                        SleepPhase::Waking,
                        SleepPhase::Awake,
                        1.0,
                        state.settings.fade_duration_seconds,
                    ));
                }
            }
            UiCommand::SetBrightness(value) => {
                let value = value.clamp(0.02, 1.0);
                state.brightness.current = value;
                state.brightness.target = value;
                state.brightness.transitioning = false;
                state.brightness.manual_override = true;
            }
            UiCommand::SetSleepBrightness(value) => {
                state.brightness.sleep_target = value.clamp(0.02, 1.0);
            }
            UiCommand::SetFadeDuration(seconds) => {
                state.settings.fade_duration_seconds = seconds.clamp(0.0, 120.0);
            }
            UiCommand::SetHrMonitoring(enabled) => state.heart_rate.enabled = enabled,
            UiCommand::SetHrDeviceFilter(filter) => {
                state.heart_rate.device_filter = filter
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
            }
            UiCommand::SetHrThreshold(bpm) => {
                state.heart_rate.threshold_bpm = bpm.clamp(20, 240);
                hr_detector_config = Some(detector_config_from_state(&state));
            }
            UiCommand::SetHrCalmPeriod(seconds) => {
                state.heart_rate.calm_period_seconds = seconds.clamp(10, 3_600);
                hr_detector_config = Some(detector_config_from_state(&state));
            }
            UiCommand::SetHrCooldown(seconds) => {
                state.heart_rate.cooldown_seconds = seconds.min(24 * 60 * 60);
                hr_detector_config = Some(detector_config_from_state(&state));
            }
            UiCommand::SetSleepDetection(enabled) => state.sleep.detection_enabled = enabled,
            UiCommand::SetOscAutomation(enabled) => state.osc.enabled = enabled,
            UiCommand::SetGogoAutomation(enabled) => state.settings.gogo_automation_enabled = enabled,
            UiCommand::SetVrchatStatusAutomation(enabled) => {
                state.settings.vrchat_status_automation_enabled = enabled;
            }
            UiCommand::SetVrchatSleepStatus(status) => state.settings.sleep_vrchat_status = status,
            UiCommand::SetVrchatWakeStatus(status) => state.settings.wake_vrchat_status = status,
            UiCommand::SetInviteAutomation(enabled) => state.invites.enabled = enabled,
            UiCommand::SetInviteProfile(profile) => state.invites.profile = profile,
            UiCommand::SetInviteRules(rules) => state.invites.rules = rules,
            UiCommand::SetOverlaySpawnDistance(distance_m) => {
                state.overlay.spawn_distance_m = distance_m.clamp(0.25, 5.0);
            }
            UiCommand::SetOverlaySpawnMode(mode) => state.overlay.spawn_mode = mode,
            UiCommand::SetOverlayLaserEnabled(enabled) => state.overlay.laser_enabled = enabled,
            UiCommand::ShowOverlay => state.overlay.visible = true,
            UiCommand::HideOverlay => state.overlay.visible = false,
        }

        if let Some(config) = hr_detector_config {
            self.hr_detector.lock().await.set_config(config);
        }

        if *state != before {
            self.emit_changes(&before, &state);
        }
        if persist {
            self.config_updates
                .send_replace(PersistedConfig::from_state(&state));
        }
        drop(state);

        if manual_wake {
            self.hr_detector
                .lock()
                .await
                .note_sleep_disabled(self.now_ms());
        }

        if let Some((transition_id, expected_phase, final_phase, final_brightness, seconds)) = transition {
            let store = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs_f32(seconds.max(0.0))).await;
                store
                    .complete_transition(
                        transition_id,
                        expected_phase,
                        final_phase,
                        final_brightness,
                    )
                    .await;
            });
        }
    }
}
