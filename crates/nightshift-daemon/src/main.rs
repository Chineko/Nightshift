mod config;

use std::{
    env,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use config::PersistedConfig;
use nightshift_protocol::{
    AppState, Capability, ClientMessage, PROTOCOL_VERSION, ServerMessage, SleepPhase, StateEvent,
    UiCommand,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Mutex, broadcast, mpsc},
};
use tracing::{info, warn};

#[derive(Clone)]
struct StateStore {
    state: Arc<Mutex<AppState>>,
    updates: broadcast::Sender<StateEvent>,
    overlay_clients: Arc<AtomicUsize>,
    config_updates: mpsc::UnboundedSender<PersistedConfig>,
}

impl StateStore {
    fn new(initial_state: AppState) -> Self {
        let (updates, _) = broadcast::channel(64);
        let (config_updates, mut config_rx) = mpsc::unbounded_channel::<PersistedConfig>();

        tokio::spawn(async move {
            while let Some(config) = config_rx.recv().await {
                if let Err(error) = config.save().await {
                    warn!(%error, "failed to persist Nightshift settings");
                }
            }
        });

        Self {
            state: Arc::new(Mutex::new(initial_state)),
            updates,
            overlay_clients: Arc::new(AtomicUsize::new(0)),
            config_updates,
        }
    }

    async fn snapshot_and_subscribe(&self) -> (AppState, broadcast::Receiver<StateEvent>) {
        let state = self.state.lock().await;
        let updates = self.updates.subscribe();
        (state.clone(), updates)
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

    async fn complete_transition(
        &self,
        expected_phase: SleepPhase,
        final_phase: SleepPhase,
        final_brightness: f32,
    ) {
        let mut state = self.state.lock().await;
        if state.sleep.phase != expected_phase {
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
                | UiCommand::SetSleepDetection(_)
                | UiCommand::SetOscAutomation(_)
                | UiCommand::SetGogoAutomation(_)
                | UiCommand::SetVrchatSleepStatus(_)
                | UiCommand::SetVrchatWakeStatus(_)
                | UiCommand::SetInviteAutomation(_)
                | UiCommand::SetInviteProfile(_)
        );

        let mut state = self.state.lock().await;
        let before = state.clone();
        let mut transition = None;

        match command {
            UiCommand::EnterSleep => {
                state.sleep.phase = SleepPhase::EnteringSleep;
                state.brightness.target = state.brightness.sleep_target;
                state.brightness.transitioning = true;
                state.brightness.manual_override = false;
                transition = Some((
                    SleepPhase::EnteringSleep,
                    SleepPhase::Sleeping,
                    state.brightness.sleep_target,
                    state.settings.fade_duration_seconds,
                ));
            }
            UiCommand::Wake => {
                state.sleep.phase = SleepPhase::Waking;
                state.brightness.target = 1.0;
                state.brightness.transitioning = true;
                state.brightness.manual_override = false;
                transition = Some((
                    SleepPhase::Waking,
                    SleepPhase::Awake,
                    1.0,
                    state.settings.fade_duration_seconds,
                ));
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
            UiCommand::SetSleepDetection(enabled) => state.sleep.detection_enabled = enabled,
            UiCommand::SetOscAutomation(enabled) => state.osc.enabled = enabled,
            UiCommand::SetGogoAutomation(enabled) => state.settings.gogo_automation_enabled = enabled,
            UiCommand::SetVrchatSleepStatus(status) => state.settings.sleep_vrchat_status = status,
            UiCommand::SetVrchatWakeStatus(status) => state.settings.wake_vrchat_status = status,
            UiCommand::SetInviteAutomation(enabled) => state.invites.enabled = enabled,
            UiCommand::SetInviteProfile(profile) => state.invites.profile = profile,
            UiCommand::ShowOverlay => state.overlay.visible = true,
            UiCommand::HideOverlay => state.overlay.visible = false,
        }

        if *state != before {
            self.emit_changes(&before, &state);
        }
        if persist {
            let config = PersistedConfig::from_state(&state);
            if self.config_updates.send(config).is_err() {
                warn!("Nightshift config writer stopped unexpectedly");
            }
        }
        drop(state);

        if let Some((expected_phase, final_phase, final_brightness, seconds)) = transition {
            let store = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs_f32(seconds.max(0.0))).await;
                store
                    .complete_transition(expected_phase, final_phase, final_brightness)
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
                info!("Nightshift overlay runtime disconnected");
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

enum ClientStep {
    Continue,
    Resync,
    Break,
}

async fn handle_client(stream: UnixStream, store: StateStore) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    if reader.read_line(&mut line).await? == 0 {
        return Ok(());
    }

    let hello: ClientMessage = serde_json::from_str(line.trim_end())?;
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

    loop {
        line.clear();
        let step = tokio::select! {
            read = reader.read_line(&mut line) => {
                if read? == 0 {
                    ClientStep::Break
                } else {
                    match serde_json::from_str::<ClientMessage>(line.trim_end()) {
                        Ok(ClientMessage::GetState) => ClientStep::Resync,
                        Ok(ClientMessage::Command { command }) => {
                            store.apply(command).await;
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

    let path = socket_path()?;
    let parent = path.parent().context("socket path has no parent")?;
    tokio::fs::create_dir_all(parent).await?;

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
            _ = tokio::signal::ctrl_c() => {
                info!("shutting down");
                break;
            }
        }
    }

    let _ = tokio::fs::remove_file(&path).await;
    Ok(())
}
