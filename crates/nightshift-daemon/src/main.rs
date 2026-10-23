use std::{env, path::PathBuf, sync::Arc};

use anyhow::{Context, Result, bail};
use nightshift_protocol::{
    AppState, Capability, ClientMessage, PROTOCOL_VERSION, ServerMessage, SleepPhase, StateEvent,
    UiCommand,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Mutex, watch},
};
use tracing::{info, warn};

#[derive(Clone)]
struct StateStore {
    state: Arc<Mutex<AppState>>,
    updates: watch::Sender<AppState>,
}

impl StateStore {
    fn new() -> Self {
        let state = AppState::default();
        let (updates, _) = watch::channel(state.clone());
        Self {
            state: Arc::new(Mutex::new(state)),
            updates,
        }
    }

    async fn snapshot(&self) -> AppState {
        self.state.lock().await.clone()
    }

    async fn apply(&self, command: UiCommand) {
        let mut state = self.state.lock().await;
        let before = state.clone();

        match command {
            UiCommand::EnterSleep => {
                state.sleep.phase = SleepPhase::EnteringSleep;
                state.brightness.target = state.brightness.sleep_target;
                state.brightness.transitioning = true;
                state.brightness.manual_override = false;
            }
            UiCommand::Wake => {
                state.sleep.phase = SleepPhase::Waking;
                state.brightness.target = 1.0;
                state.brightness.transitioning = true;
                state.brightness.manual_override = false;
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
            UiCommand::SetHrMonitoring(enabled) => state.heart_rate.enabled = enabled,
            UiCommand::SetSleepDetection(enabled) => state.sleep.detection_enabled = enabled,
            UiCommand::SetOscAutomation(enabled) => state.osc.enabled = enabled,
            UiCommand::SetVrchatSleepStatus(status) | UiCommand::SetVrchatWakeStatus(status) => {
                state.vrchat.status = status;
            }
            UiCommand::SetInviteAutomation(enabled) => state.invites.enabled = enabled,
            UiCommand::SetInviteProfile(profile) => state.invites.profile = profile,
            UiCommand::ShowOverlay => state.overlay.visible = true,
            UiCommand::HideOverlay => state.overlay.visible = false,
        }

        if *state != before {
            self.updates.send_replace(state.clone());
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

async fn handle_client(stream: UnixStream, store: StateStore) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    if reader.read_line(&mut line).await? == 0 {
        return Ok(());
    }

    let hello: ClientMessage = serde_json::from_str(line.trim_end())?;
    let ClientMessage::Hello { protocol, .. } = hello else {
        bail!("first client message must be hello");
    };

    if protocol != PROTOCOL_VERSION {
        send_message(
            &mut write_half,
            &ServerMessage::Error {
                message: format!(
                    "protocol mismatch: client={protocol}, server={PROTOCOL_VERSION}"
                ),
            },
        )
        .await?;
        return Ok(());
    }

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

    send_message(
        &mut write_half,
        &ServerMessage::FullState {
            state: store.snapshot().await,
        },
    )
    .await?;

    let mut updates = store.updates.subscribe();

    loop {
        line.clear();
        tokio::select! {
            read = reader.read_line(&mut line) => {
                if read? == 0 {
                    break;
                }

                match serde_json::from_str::<ClientMessage>(line.trim_end()) {
                    Ok(ClientMessage::GetState) => {
                        send_message(
                            &mut write_half,
                            &ServerMessage::FullState { state: store.snapshot().await },
                        ).await?;
                    }
                    Ok(ClientMessage::Command { command }) => store.apply(command).await,
                    Ok(ClientMessage::Hello { .. }) => {
                        send_message(
                            &mut write_half,
                            &ServerMessage::Error { message: "hello already completed".into() },
                        ).await?;
                    }
                    Err(error) => {
                        send_message(
                            &mut write_half,
                            &ServerMessage::Error { message: error.to_string() },
                        ).await?;
                    }
                }
            }
            changed = updates.changed() => {
                if changed.is_err() {
                    break;
                }
                let state = updates.borrow_and_update().clone();
                send_message(
                    &mut write_half,
                    &ServerMessage::Event { event: StateEvent::FullState(state) },
                ).await?;
            }
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
        tokio::fs::remove_file(&path).await?;
    }

    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind {}", path.display()))?;
    info!(path = %path.display(), "Nightshift IPC ready");

    let store = StateStore::new();

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
