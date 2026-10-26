mod config;
mod ipc;
mod state;

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
use config::{PersistedConfig, sanitize_overlay_pose};
use nightshift_hr::{CalmPeriodConfig, CalmPeriodDetector, DetectionState};
use nightshift_protocol::{
    AppState, Capability, ClientMessage, HrDeviceInfo, HrProviderStatus, HrStatus, InviteAction,
    InviteActionKind, OverlayPose, OverlaySpawnMode, PROTOCOL_VERSION, RecentEvent, RecentEventKind,
    ServerMessage, SleepPhase, SleepingPose, StateEvent, UiCommand, VrchatStatus, WorkerReport,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    process::Command,
    sync::{Mutex, broadcast, mpsc, watch},
};
use tracing::{info, warn};

const CONFIG_WRITE_DEBOUNCE: Duration = Duration::from_millis(250);
const MAX_HR_DISCOVERED_DEVICES: usize = 32;
const MAX_IPC_LINE_BYTES: usize = 64 * 1024;
const VRCHAT_WORKER_RESTART_DELAY: Duration = Duration::from_secs(1);

use ipc::*;
use state::*;

fn vrchat_worker_program() -> PathBuf {
    if let Ok(current_exe) = env::current_exe()
        && let Some(parent) = current_exe.parent()
    {
        let sibling = parent.join("nightshift-vrchat");
        if sibling.is_file() {
            return sibling;
        }
    }

    PathBuf::from("nightshift-vrchat")
}

async fn supervise_vrchat_worker(vrchat_clients: Arc<AtomicUsize>) {
    loop {
        if vrchat_clients.load(Ordering::Acquire) > 0 {
            tokio::time::sleep(VRCHAT_WORKER_RESTART_DELAY).await;
            continue;
        }

        let program = vrchat_worker_program();
        let mut command = Command::new(&program);
        command.kill_on_drop(true);

        match command.spawn() {
            Ok(mut child) => {
                info!(program = %program.display(), "started managed VRChat worker");
                match child.wait().await {
                    Ok(status) => {
                        warn!(%status, "managed VRChat worker exited; restarting");
                    }
                    Err(error) => {
                        warn!(%error, "failed while waiting for managed VRChat worker; restarting");
                    }
                }
            }
            Err(error) => {
                warn!(
                    %error,
                    program = %program.display(),
                    "failed to start managed VRChat worker; build/install nightshift-vrchat and retrying"
                );
            }
        }

        tokio::time::sleep(VRCHAT_WORKER_RESTART_DELAY).await;
    }
}

#[tokio::main(flavor = "current_thread")]
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
    let vrchat_supervisor = tokio::spawn(supervise_vrchat_worker(store.vrchat_clients.clone()));

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

    vrchat_supervisor.abort();
    let _ = vrchat_supervisor.await;

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
mod tests;
