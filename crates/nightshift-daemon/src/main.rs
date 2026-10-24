mod config;

use std::{
    env,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use config::PersistedConfig;
use nightshift_hr::{CalmPeriodConfig, CalmPeriodDetector, DetectionState};
use nightshift_protocol::{
    AppState, Capability, ClientMessage, HrDeviceInfo, HrProviderStatus, HrStatus, InviteAction,
    InviteActionKind, OverlaySpawnMode, PROTOCOL_VERSION, ServerMessage, SleepPhase, SleepingPose,
    StateEvent, UiCommand, VrchatStatus, WorkerReport,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{Mutex, broadcast, mpsc, watch},
};
use tracing::{info, warn};

const CONFIG_WRITE_DEBOUNCE: Duration = Duration::from_millis(250);
const MAX_HR_DISCOVERED_DEVICES: usize = 32;
const MAX_IPC_LINE_BYTES: usize = 64 * 1024;

fn detector_config_from_state(state: &AppState) -> CalmPeriodConfig {
    CalmPeriodConfig {
        heart_rate_threshold_bpm: state.heart_rate.threshold_bpm,
        calm_period_ms: state.heart_rate.calm_period_seconds.saturating_mul(1_000),
        manual_wake_cooldown_ms: state.heart_rate.cooldown_seconds.saturating_mul(1_000),
        ..CalmPeriodConfig::default()
    }
}

fn normalize_hr_devices(devices: Vec<HrDeviceInfo>) -> Vec<HrDeviceInfo> {
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
struct StateStore {
    state: Arc<Mutex<AppState>>,
    updates: broadcast::Sender<StateEvent>,
    overlay_clients: Arc<AtomicUsize>,
    osc_clients: Arc<AtomicUsize>,
    hr_clients: Arc<AtomicUsize>,
    vrchat_clients: Arc<AtomicUsize>,
    transition_generation: Arc<AtomicU64>,
    config_updates: watch::Sender<PersistedConfig>,
    config_save_lock: Arc<Mutex<()>>,
    config_writer_shutdown: Arc<AtomicBool>,
    hr_detector: Arc<Mutex<CalmPeriodDetector>>,
    started: Instant,
}

impl StateStore {
    fn new(initial_state: AppState) -> Self {
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

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    async fn snapshot_and_subscribe(&self) -> (AppState, broadcast::Receiver<StateEvent>) {
        let state = self.state.lock().await;
        let updates = self.updates.subscribe();
        (state.clone(), updates)
    }

    async fn flush_config(&self) -> Result<()> {
        self.config_writer_shutdown.store(true, Ordering::Release);
        let config = {
            let state = self.state.lock().await;
            PersistedConfig::from_state(&state)
        };
        let _save_guard = self.config_save_lock.lock().await;
        config.save().await
    }

    fn emit_changes(&self, before: &AppState, after: &AppState) {
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

    async fn set_overlay_runtime(&self, connected: bool, client_version: Option<String>) {
        let mut state = self.state.lock().await;
        let before = state.clone();
        state.overlay.runtime_connected = connected;
        state.overlay.runtime_client_version = if connected { client_version } else { None };

        if *state != before {
            self.emit_changes(&before, &state);
        }
    }

    async fn set_sleeping_pose(&self, pose: SleepingPose) {
        let mut state = self.state.lock().await;
        let before = state.clone();
        state.sleep.pose = pose;

        if *state != before {
            self.emit_changes(&before, &state);
        }
    }

    async fn set_osc_runtime(
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

    async fn set_vrchat_runtime(
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

    async fn set_hr_devices(&self, devices: Vec<HrDeviceInfo>) {
        let mut state = self.state.lock().await;
        let before = state.clone();
        state.heart_rate.available_devices = normalize_hr_devices(devices);
        if *state != before {
            self.emit_changes(&before, &state);
        }
    }

    async fn set_hr_provider_runtime(
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

    async fn set_hr_runtime(&self, connected: bool, bpm: Option<u16>) {
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

    async fn apply_worker_report(&self, client: &str, report: WorkerReport) -> bool {
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

    async fn complete_transition(
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

    async fn apply(&self, command: UiCommand) {
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

struct OverlayClientGuard {
    store: StateStore,
}

impl Drop for OverlayClientGuard {
    fn drop(&mut self) {
        let previous = self.store.overlay_clients.fetch_sub(1, Ordering::AcqRel);
        if previous == 1 {
            let store = self.store.clone();
            tokio::spawn(async move {
                store.set_overlay_runtime(false, None).await;
                store.set_sleeping_pose(SleepingPose::Unknown).await;
                info!("Nightshift overlay runtime disconnected");
            });
        }
    }
}

struct OscClientGuard {
    store: StateStore,
}

impl Drop for OscClientGuard {
    fn drop(&mut self) {
        let previous = self.store.osc_clients.fetch_sub(1, Ordering::AcqRel);
        if previous == 1 {
            let store = self.store.clone();
            tokio::spawn(async move {
                store.set_osc_runtime(false, None, 0, false).await;
                info!("Nightshift OSC worker disconnected");
            });
        }
    }
}

struct HrClientGuard {
    store: StateStore,
}

impl Drop for HrClientGuard {
    fn drop(&mut self) {
        let previous = self.store.hr_clients.fetch_sub(1, Ordering::AcqRel);
        if previous == 1 {
            let store = self.store.clone();
            tokio::spawn(async move {
                store.set_hr_runtime(false, None).await;
                store
                    .set_hr_provider_runtime(HrProviderStatus::Unknown, None, None)
                    .await;
                info!("Nightshift heart-rate worker disconnected");
            });
        }
    }
}

struct VrchatClientGuard {
    store: StateStore,
}

impl Drop for VrchatClientGuard {
    fn drop(&mut self) {
        let previous = self.store.vrchat_clients.fetch_sub(1, Ordering::AcqRel);
        if previous == 1 {
            let store = self.store.clone();
            tokio::spawn(async move {
                store
                    .set_vrchat_runtime(false, None, VrchatStatus::Offline, None, None)
                    .await;
                info!("Nightshift VRChat worker disconnected");
            });
        }
    }
}

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

async fn send_message(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: &ServerMessage,
) -> Result<()> {
    let mut json = serde_json::to_vec(message)?;
    json.push(b'\n');
    writer.write_all(&json).await?;
    Ok(())
}

async fn read_ipc_line<R>(reader: &mut R, buffer: &mut Vec<u8>) -> Result<Option<String>>
where
    R: AsyncRead + Unpin,
{
    loop {
        if let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            if newline > MAX_IPC_LINE_BYTES {
                bail!("IPC message exceeds {MAX_IPC_LINE_BYTES} byte limit");
            }
            let mut bytes: Vec<u8> = buffer.drain(..=newline).collect();
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            let line = String::from_utf8(bytes).context("IPC message is not valid UTF-8")?;
            return Ok(Some(line));
        }

        if buffer.len() > MAX_IPC_LINE_BYTES {
            bail!("IPC message exceeds {MAX_IPC_LINE_BYTES} byte limit");
        }

        let read = reader.read_buf(buffer).await?;
        if read == 0 {
            if buffer.is_empty() {
                return Ok(None);
            }
            if buffer.len() > MAX_IPC_LINE_BYTES {
                bail!("IPC message exceeds {MAX_IPC_LINE_BYTES} byte limit");
            }
            let bytes = std::mem::take(buffer);
            let line = String::from_utf8(bytes).context("IPC message is not valid UTF-8")?;
            return Ok(Some(line));
        }
    }
}

enum ClientStep {
    Continue,
    Resync,
    Break,
}

async fn handle_client(stream: UnixStream, store: StateStore) -> Result<()> {
    let (mut read_half, mut write_half) = stream.into_split();
    let mut read_buffer = Vec::with_capacity(4096);

    let Some(line) = read_ipc_line(&mut read_half, &mut read_buffer).await? else {
        return Ok(());
    };

    let hello: ClientMessage = serde_json::from_str(&line)?;
    let ClientMessage::Hello {
        protocol,
        client,
        client_version,
    } = hello
    else {
        bail!("first client message must be hello");
    };

    if protocol != PROTOCOL_VERSION {
        send_message(
            &mut write_half,
            &ServerMessage::Error {
                message: format!("protocol mismatch: client={protocol}, server={PROTOCOL_VERSION}"),
            },
        )
        .await?;
        return Ok(());
    }

    let is_overlay = matches!(
        client.as_str(),
        "nightshift-overlay" | "nightshift-overlay-prototype"
    );
    let _overlay_guard = if is_overlay {
        let previous = store.overlay_clients.fetch_add(1, Ordering::AcqRel);
        store
            .set_overlay_runtime(true, Some(client_version.clone()))
            .await;
        if previous == 0 {
            info!(version = %client_version, "Nightshift overlay runtime connected");
        }
        Some(OverlayClientGuard {
            store: store.clone(),
        })
    } else {
        None
    };

    let is_osc_worker = client == "nightshift-osc";
    let _osc_guard = if is_osc_worker {
        store.osc_clients.fetch_add(1, Ordering::AcqRel);
        Some(OscClientGuard {
            store: store.clone(),
        })
    } else {
        None
    };

    let is_hr_worker = client == "nightshift-hr";
    let _hr_guard = if is_hr_worker {
        store.hr_clients.fetch_add(1, Ordering::AcqRel);
        Some(HrClientGuard {
            store: store.clone(),
        })
    } else {
        None
    };

    let is_vrchat_worker = client == "nightshift-vrchat";
    let _vrchat_guard = if is_vrchat_worker {
        store.vrchat_clients.fetch_add(1, Ordering::AcqRel);
        Some(VrchatClientGuard {
            store: store.clone(),
        })
    } else {
        None
    };

    send_message(
        &mut write_half,
        &ServerMessage::Hello {
            protocol: PROTOCOL_VERSION,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            capabilities: vec![
                Capability::Sleep,
                Capability::Brightness,
                Capability::HeartRate,
                Capability::Osc,
                Capability::VrchatStatus,
                Capability::InviteRules,
                Capability::Overlay,
                Capability::WorkerReports,
            ],
        },
    )
    .await?;

    let (snapshot, mut updates) = store.snapshot_and_subscribe().await;
    send_message(
        &mut write_half,
        &ServerMessage::FullState { state: snapshot },
    )
    .await?;

    let (line_tx, mut line_rx) = mpsc::channel::<std::result::Result<String, String>>(16);
    tokio::spawn(async move {
        let mut buffer = read_buffer;
        loop {
            match read_ipc_line(&mut read_half, &mut buffer).await {
                Ok(Some(line)) => {
                    if line_tx.send(Ok(line)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = line_tx.send(Err(error.to_string())).await;
                    break;
                }
            }
        }
    });

    loop {
        let step = tokio::select! {
            read = line_rx.recv() => {
                match read {
                    None => ClientStep::Break,
                    Some(Err(error)) => {
                        warn!(%error, client = %client, "rejecting Nightshift IPC client input");
                        ClientStep::Break
                    }
                    Some(Ok(line)) => {
                        match serde_json::from_str::<ClientMessage>(&line) {
                            Ok(ClientMessage::GetState) => ClientStep::Resync,
                            Ok(ClientMessage::Command { command }) => {
                                store.apply(command).await;
                                ClientStep::Continue
                            }
                            Ok(ClientMessage::Report { report }) => {
                                if !store.apply_worker_report(&client, report).await {
                                    send_message(
                                        &mut write_half,
                                        &ServerMessage::Error {
                                            message: "client is not authorized for this worker report".into(),
                                        },
                                    ).await?;
                                }
                                ClientStep::Continue
                            }
                            Ok(ClientMessage::Hello { .. }) => {
                                send_message(
                                    &mut write_half,
                                    &ServerMessage::Error { message: "hello already completed".into() },
                                ).await?;
                                ClientStep::Continue
                            }
                            Err(error) => {
                                send_message(
                                    &mut write_half,
                                    &ServerMessage::Error { message: error.to_string() },
                                ).await?;
                                ClientStep::Continue
                            }
                        }
                    }
                }
            }
            event = updates.recv() => {
                match event {
                    Ok(event) => {
                        send_message(&mut write_half, &ServerMessage::Event { event }).await?;
                        ClientStep::Continue
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => ClientStep::Resync,
                    Err(broadcast::error::RecvError::Closed) => ClientStep::Break,
                }
            }
        };

        match step {
            ClientStep::Continue => {}
            ClientStep::Resync => {
                let (snapshot, new_updates) = store.snapshot_and_subscribe().await;
                updates = new_updates;
                send_message(
                    &mut write_half,
                    &ServerMessage::FullState { state: snapshot },
                )
                .await?;
            }
            ClientStep::Break => break,
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nightshift_daemon=info".into()),
        )
        .init();

    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("installing SIGTERM handler")?;

    let path = socket_path()?;
    let parent = path.parent().context("socket path has no parent")?;
    tokio::fs::create_dir_all(parent).await?;
    tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
        .await
        .with_context(|| format!("securing runtime directory {}", parent.display()))?;

    if tokio::fs::try_exists(&path).await? {
        match UnixStream::connect(&path).await {
            Ok(_) => bail!("Nightshift daemon is already running at {}", path.display()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                ) =>
            {
                tokio::fs::remove_file(&path).await?;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to validate existing socket {}", path.display()));
            }
        }
    }

    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind {}", path.display()))?;
    tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .await
        .with_context(|| format!("securing IPC socket {}", path.display()))?;
    info!(path = %path.display(), "Nightshift IPC ready");

    let mut initial_state = AppState::default();
    match PersistedConfig::load().await {
        Ok(config) => {
            config.apply_to_state(&mut initial_state);
            info!("loaded persisted Nightshift settings");
        }
        Err(error) => {
            warn!(%error, "failed to load Nightshift settings; using defaults");
        }
    }
    let store = StateStore::new(initial_state);

    let run_result: Result<()> = async {
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    let (stream, _) = accept?;
                    let store = store.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle_client(stream, store).await {
                            warn!(%error, "Nightshift IPC client disconnected with an error");
                        }
                    });
                }
                result = tokio::signal::ctrl_c() => {
                    result.context("waiting for Ctrl-C")?;
                    info!(signal = "SIGINT", "shutting down");
                    break;
                }
                signal = terminate.recv() => {
                    if signal.is_none() {
                        bail!("SIGTERM signal stream closed");
                    }
                    info!(signal = "SIGTERM", "shutting down");
                    break;
                }
            }
        }
        Ok(())
    }
    .await;

    if let Err(error) = store.flush_config().await {
        warn!(%error, "failed to flush Nightshift settings during shutdown");
    }
    if let Err(error) = tokio::fs::remove_file(&path).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        warn!(%error, path = %path.display(), "failed to remove Nightshift IPC socket during shutdown");
    }

    run_result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ipc_line_reader_preserves_multiple_frames() {
        let (mut writer, mut reader) = tokio::io::duplex(128);
        let write_task = tokio::spawn(async move {
            writer.write_all(b"first\nsecond\r\n").await.unwrap();
        });
        let mut buffer = Vec::new();

        assert_eq!(
            read_ipc_line(&mut reader, &mut buffer).await.unwrap().as_deref(),
            Some("first")
        );
        assert_eq!(
            read_ipc_line(&mut reader, &mut buffer).await.unwrap().as_deref(),
            Some("second")
        );
        write_task.await.unwrap();
    }

    #[tokio::test]
    async fn ipc_line_reader_rejects_oversized_frames() {
        let (mut writer, mut reader) = tokio::io::duplex(MAX_IPC_LINE_BYTES + 2);
        let write_task = tokio::spawn(async move {
            writer
                .write_all(&vec![b'x'; MAX_IPC_LINE_BYTES + 1])
                .await
                .unwrap();
        });
        let mut buffer = Vec::new();

        let error = read_ipc_line(&mut reader, &mut buffer).await.unwrap_err();
        assert!(error.to_string().contains("exceeds"));
        write_task.await.unwrap();
    }

    #[tokio::test]
    async fn repeated_same_transition_is_idempotent() {
        let store = StateStore::new(AppState::default());
        store.apply(UiCommand::EnterSleep).await;
        let first = store.transition_generation.load(Ordering::Acquire);

        store.apply(UiCommand::EnterSleep).await;
        let second = store.transition_generation.load(Ordering::Acquire);

        assert_eq!(first, second);
        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.sleep.phase, SleepPhase::EnteringSleep);
    }

    #[tokio::test]
    async fn stale_timer_cannot_finish_reentered_phase() {
        let store = StateStore::new(AppState::default());
        store.apply(UiCommand::EnterSleep).await;
        let stale = store.transition_generation.load(Ordering::Acquire);

        store.apply(UiCommand::Wake).await;
        store.apply(UiCommand::EnterSleep).await;
        let current = store.transition_generation.load(Ordering::Acquire);
        assert_ne!(stale, current);

        store
            .complete_transition(stale, SleepPhase::EnteringSleep, SleepPhase::Sleeping, 0.06)
            .await;

        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.sleep.phase, SleepPhase::EnteringSleep);
        assert!(snapshot.brightness.transitioning);
    }

    #[tokio::test]
    async fn overlay_settings_are_clamped_and_applied() {
        let store = StateStore::new(AppState::default());

        store.apply(UiCommand::SetOverlaySpawnDistance(99.0)).await;
        store
            .apply(UiCommand::SetOverlaySpawnMode(
                OverlaySpawnMode::RestoreLastPosition,
            ))
            .await;

        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.overlay.spawn_distance_m, 5.0);
        assert_eq!(
            snapshot.overlay.spawn_mode,
            OverlaySpawnMode::RestoreLastPosition
        );

        store.apply(UiCommand::SetOverlaySpawnDistance(0.0)).await;
        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.overlay.spawn_distance_m, 0.25);
    }

    #[tokio::test]
    async fn hr_settings_are_clamped_and_reconfigure_detector() {
        let store = StateStore::new(AppState::default());

        store.apply(UiCommand::SetHrDeviceFilter(Some("  Polar H10  ".into()))).await;
        store.apply(UiCommand::SetHrThreshold(999)).await;
        store.apply(UiCommand::SetHrCalmPeriod(1)).await;
        store.apply(UiCommand::SetHrCooldown(u64::MAX)).await;

        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.heart_rate.device_filter.as_deref(), Some("Polar H10"));
        assert_eq!(snapshot.heart_rate.threshold_bpm, 240);
        assert_eq!(snapshot.heart_rate.calm_period_seconds, 10);
        assert_eq!(snapshot.heart_rate.cooldown_seconds, 24 * 60 * 60);

        let config = store.hr_detector.lock().await.config();
        assert_eq!(config.heart_rate_threshold_bpm, 240);
        assert_eq!(config.calm_period_ms, 10_000);
        assert_eq!(config.manual_wake_cooldown_ms, 24 * 60 * 60 * 1_000);

        store.apply(UiCommand::SetHrDeviceFilter(Some("   ".into()))).await;
        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.heart_rate.device_filter, None);
    }

    #[tokio::test]
    async fn persisted_hr_tuning_initializes_detector() {
        let mut state = AppState::default();
        state.heart_rate.threshold_bpm = 72;
        state.heart_rate.calm_period_seconds = 45;
        state.heart_rate.cooldown_seconds = 300;
        let store = StateStore::new(state);

        let config = store.hr_detector.lock().await.config();
        assert_eq!(config.heart_rate_threshold_bpm, 72);
        assert_eq!(config.calm_period_ms, 45_000);
        assert_eq!(config.manual_wake_cooldown_ms, 300_000);
    }

    #[test]
    fn hr_device_snapshots_are_bounded_normalized_and_deduplicated() {
        let mut devices = vec![
            HrDeviceInfo {
                id: " same ".into(),
                address: " AA ".into(),
                name: Some(" Polar ".into()),
            },
            HrDeviceInfo {
                id: "same".into(),
                address: "BB".into(),
                name: None,
            },
        ];
        devices.extend((0..40).map(|n| HrDeviceInfo {
            id: format!("id-{n}"),
            address: format!("addr-{n}"),
            name: None,
        }));

        let devices = normalize_hr_devices(devices);
        assert_eq!(devices.len(), MAX_HR_DISCOVERED_DEVICES);
        assert_eq!(devices.iter().filter(|device| device.id == "same").count(), 1);
        assert_eq!(devices[0].id, "same");
        assert_eq!(devices[0].address, "AA");
        assert_eq!(devices[0].name.as_deref(), Some("Polar"));
    }

    #[tokio::test]
    async fn hr_provider_reports_are_worker_scoped_and_manage_health_state() {
        let store = StateStore::new(AppState::default());
        assert!(
            store
                .apply_worker_report(
                    "nightshift-hr",
                    WorkerReport::HeartRateDevices {
                        devices: vec![HrDeviceInfo {
                            id: "polar".into(),
                            address: "AA:BB".into(),
                            name: Some("Polar H10".into()),
                        }],
                    },
                )
                .await
        );
        assert!(
            !store
                .apply_worker_report(
                    "nightshift-osc",
                    WorkerReport::HeartRateDevices { devices: vec![] },
                )
                .await
        );
        assert!(
            store
                .apply_worker_report(
                    "nightshift-hr",
                    WorkerReport::HeartRateProviderRuntime {
                        status: HrProviderStatus::Error,
                        connected_device: None,
                        last_error: Some("preferred device not found".into()),
                    },
                )
                .await
        );

        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.heart_rate.provider_status, HrProviderStatus::Error);
        assert_eq!(snapshot.heart_rate.last_error.as_deref(), Some("preferred device not found"));
        assert_eq!(snapshot.heart_rate.available_devices.len(), 1);

        store
            .apply_worker_report(
                "nightshift-hr",
                WorkerReport::HeartRateProviderRuntime {
                    status: HrProviderStatus::Searching,
                    connected_device: None,
                    last_error: None,
                },
            )
            .await;
        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.heart_rate.last_error.as_deref(), Some("preferred device not found"));
        assert!(snapshot.heart_rate.available_devices.is_empty());

        store
            .apply_worker_report(
                "nightshift-hr",
                WorkerReport::HeartRateProviderRuntime {
                    status: HrProviderStatus::Connected,
                    connected_device: Some("Polar H10".into()),
                    last_error: None,
                },
            )
            .await;
        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.heart_rate.provider_status, HrProviderStatus::Connected);
        assert_eq!(snapshot.heart_rate.connected_device.as_deref(), Some("Polar H10"));
        assert_eq!(snapshot.heart_rate.last_error, None);
    }

    #[tokio::test]
    async fn osc_worker_report_updates_runtime_state() {
        let store = StateStore::new(AppState::default());
        assert!(
            store
                .apply_worker_report(
                    "nightshift-osc",
                    WorkerReport::OscRuntime {
                        connected: true,
                        avatar_id: Some("avtr_test".into()),
                        avatar_parameter_count: 42,
                        gogo_detected: true,
                    },
                )
                .await
        );

        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert!(snapshot.osc.connected);
        assert_eq!(snapshot.osc.avatar_id.as_deref(), Some("avtr_test"));
        assert_eq!(snapshot.osc.avatar_parameter_count, 42);
        assert!(snapshot.osc.gogo_detected);
    }

    #[tokio::test]
    async fn overlay_can_report_pose_but_osc_worker_cannot() {
        let store = StateStore::new(AppState::default());
        assert!(
            store
                .apply_worker_report(
                    "nightshift-overlay",
                    WorkerReport::SleepingPose {
                        pose: SleepingPose::Left,
                    },
                )
                .await
        );
        assert!(
            !store
                .apply_worker_report(
                    "nightshift-osc",
                    WorkerReport::SleepingPose {
                        pose: SleepingPose::Right,
                    },
                )
                .await
        );

        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.sleep.pose, SleepingPose::Left);
    }

    #[tokio::test]
    async fn heart_rate_reports_are_worker_scoped() {
        let store = StateStore::new(AppState::default());
        assert!(
            store
                .apply_worker_report(
                    "nightshift-hr",
                    WorkerReport::HeartRateRuntime {
                        connected: true,
                        bpm: Some(55),
                    },
                )
                .await
        );
        assert!(
            !store
                .apply_worker_report(
                    "nightshift-osc",
                    WorkerReport::HeartRateRuntime {
                        connected: true,
                        bpm: Some(90),
                    },
                )
                .await
        );

        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert!(snapshot.heart_rate.connected);
        assert_eq!(snapshot.heart_rate.bpm, Some(55));
    }

    #[tokio::test]
    async fn vrchat_reports_are_worker_scoped_and_invite_feedback_is_additive() {
        let store = StateStore::new(AppState::default());
        let accepted = InviteAction {
            kind: InviteActionKind::Accepted,
            display_name: Some("Friend".into()),
        };
        assert!(
            store
                .apply_worker_report(
                    "nightshift-vrchat",
                    WorkerReport::VrchatRuntime {
                        connected: true,
                        display_name: Some("Hydra Employee".into()),
                        status: VrchatStatus::AskMe,
                        last_invite_action: Some(accepted.clone()),
                        invite_error: None,
                    },
                )
                .await
        );
        assert!(
            !store
                .apply_worker_report(
                    "nightshift-osc",
                    WorkerReport::VrchatRuntime {
                        connected: true,
                        display_name: Some("Impostor".into()),
                        status: VrchatStatus::Busy,
                        last_invite_action: None,
                        invite_error: Some("should not apply".into()),
                    },
                )
                .await
        );

        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert!(snapshot.vrchat.connected);
        assert_eq!(snapshot.vrchat.display_name.as_deref(), Some("Hydra Employee"));
        assert_eq!(snapshot.vrchat.status, VrchatStatus::AskMe);
        assert_eq!(snapshot.invites.last_action, Some(accepted));
        assert_eq!(snapshot.invites.last_error, None);

        assert!(
            store
                .apply_worker_report(
                    "nightshift-vrchat",
                    WorkerReport::VrchatRuntime {
                        connected: true,
                        display_name: Some("Hydra Employee".into()),
                        status: VrchatStatus::AskMe,
                        last_invite_action: None,
                        invite_error: Some("network failure".into()),
                    },
                )
                .await
        );
        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.invites.last_action.as_ref().map(|action| action.kind), Some(InviteActionKind::Accepted));
        assert_eq!(snapshot.invites.last_error.as_deref(), Some("network failure"));

        assert!(
            store
                .apply_worker_report(
                    "nightshift-vrchat",
                    WorkerReport::VrchatRuntime {
                        connected: true,
                        display_name: Some("Hydra Employee".into()),
                        status: VrchatStatus::AskMe,
                        last_invite_action: None,
                        invite_error: None,
                    },
                )
                .await
        );
        let (snapshot, _) = store.snapshot_and_subscribe().await;
        assert_eq!(snapshot.invites.last_error.as_deref(), Some("network failure"));
    }
}
