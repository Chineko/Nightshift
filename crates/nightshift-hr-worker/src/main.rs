use std::{env, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use nightshift_hr_ble::{BleHeartRateEvent, discover_device, first_adapter, run_device};
use nightshift_protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, StateEvent, WorkerReport,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::mpsc,
    task::JoinHandle,
};
use tracing::{info, warn};

const DAEMON_RECONNECT_DELAY: Duration = Duration::from_secs(1);
const BLE_RETRY_DELAY: Duration = Duration::from_secs(3);

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

fn configured_device_filter() -> Option<String> {
    env::var("NIGHTSHIFT_HR_BLE_DEVICE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn write_client_message(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: &ClientMessage,
) -> Result<()> {
    let mut bytes = serde_json::to_vec(message).context("serializing daemon IPC message")?;
    bytes.push(b'\n');
    writer
        .write_all(&bytes)
        .await
        .context("writing daemon IPC message")?;
    Ok(())
}

async fn report_runtime(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    connected: bool,
    bpm: Option<u16>,
) -> Result<()> {
    write_client_message(
        writer,
        &ClientMessage::Report {
            report: WorkerReport::HeartRateRuntime { connected, bpm },
        },
    )
    .await
}

fn spawn_ble_provider(
    events: mpsc::Sender<BleHeartRateEvent>,
    device_filter: Option<String>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let result = async {
                let adapter = first_adapter().await?;
                let peripheral = discover_device(&adapter, device_filter.as_deref()).await?;
                run_device(peripheral, events.clone()).await
            }
            .await;

            if let Err(error) = result {
                warn!(%error, "BLE heart-rate provider failed; retrying");
                let _ = events.send(BleHeartRateEvent::Disconnected).await;
            }

            tokio::time::sleep(BLE_RETRY_DELAY).await;
        }
    })
}

fn set_provider_enabled(
    enabled: bool,
    provider: &mut Option<JoinHandle<()>>,
    events: &mpsc::Sender<BleHeartRateEvent>,
    device_filter: &Option<String>,
) {
    match (enabled, provider.is_some()) {
        (true, false) => {
            info!(
                filter = device_filter.as_deref().unwrap_or("<first heart-rate device>"),
                "starting BLE heart-rate provider"
            );
            *provider = Some(spawn_ble_provider(events.clone(), device_filter.clone()));
        }
        (false, true) => {
            if let Some(task) = provider.take() {
                task.abort();
            }
            info!("BLE heart-rate provider stopped");
        }
        _ => {}
    }
}

async fn run_connection(stream: UnixStream) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    write_client_message(
        &mut write_half,
        &ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            client: "nightshift-hr".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        },
    )
    .await?;

    let device_filter = configured_device_filter();
    let (event_tx, mut event_rx) = mpsc::channel::<BleHeartRateEvent>(32);
    let mut provider: Option<JoinHandle<()>> = None;
    let mut enabled = false;
    let mut line = String::new();

    loop {
        tokio::select! {
            read = reader.read_line(&mut line) => {
                if read.context("reading daemon IPC")? == 0 {
                    bail!("daemon closed the socket");
                }

                let message: ServerMessage = serde_json::from_str(line.trim_end())
                    .context("decoding daemon IPC message")?;
                line.clear();

                match message {
                    ServerMessage::Hello { protocol, .. } => {
                        if protocol != PROTOCOL_VERSION {
                            bail!(
                                "daemon protocol mismatch: hr-worker={}, daemon={protocol}",
                                PROTOCOL_VERSION
                            );
                        }
                        info!(protocol, "Nightshift heart-rate worker connected to daemon");
                    }
                    ServerMessage::FullState { state } => {
                        enabled = state.heart_rate.enabled;
                        set_provider_enabled(
                            enabled,
                            &mut provider,
                            &event_tx,
                            &device_filter,
                        );
                        if !enabled {
                            report_runtime(&mut write_half, false, None).await?;
                        }
                    }
                    ServerMessage::Event { event: StateEvent::HeartRateChanged(state) } => {
                        let new_enabled = state.enabled;
                        if new_enabled != enabled {
                            enabled = new_enabled;
                            set_provider_enabled(
                                enabled,
                                &mut provider,
                                &event_tx,
                                &device_filter,
                            );
                            if !enabled {
                                report_runtime(&mut write_half, false, None).await?;
                            }
                        }
                    }
                    ServerMessage::Error { message } => {
                        warn!(%message, "daemon rejected heart-rate worker message");
                    }
                    _ => {}
                }
            }
            event = event_rx.recv(), if enabled => {
                let Some(event) = event else {
                    bail!("BLE provider event channel closed");
                };

                match event {
                    BleHeartRateEvent::Connected(device) => {
                        info!(
                            name = device.name.as_deref().unwrap_or("<unnamed>"),
                            address = %device.address,
                            "BLE heart-rate device connected"
                        );
                        report_runtime(&mut write_half, true, None).await?;
                    }
                    BleHeartRateEvent::Measurement(bpm) => {
                        report_runtime(&mut write_half, true, Some(bpm)).await?;
                    }
                    BleHeartRateEvent::Disconnected => {
                        report_runtime(&mut write_half, false, None).await?;
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nightshift_hr_worker=info".into()),
        )
        .init();

    let socket = socket_path()?;
    loop {
        match UnixStream::connect(&socket).await {
            Ok(stream) => {
                if let Err(error) = run_connection(stream).await {
                    warn!(%error, "heart-rate worker disconnected from daemon");
                }
            }
            Err(error) => {
                warn!(%error, path = %socket.display(), "Nightshift daemon unavailable");
            }
        }

        tokio::time::sleep(DAEMON_RECONNECT_DELAY).await;
    }
}
