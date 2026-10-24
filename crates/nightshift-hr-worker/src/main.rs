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

struct ProviderTask(Option<JoinHandle<()>>);

impl ProviderTask {
    fn is_running(&self) -> bool {
        self.0.as_ref().is_some_and(|task| !task.is_finished())
    }

    fn start(&mut self, task: JoinHandle<()>) {
        self.stop();
        self.0 = Some(task);
    }

    fn stop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

impl Drop for ProviderTask {
    fn drop(&mut self) {
        self.stop();
    }
}

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

fn normalize_device_filter(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn legacy_device_filter() -> Option<String> {
    normalize_device_filter(env::var("NIGHTSHIFT_HR_BLE_DEVICE").ok())
}

fn effective_device_filter(
    configured: Option<String>,
    legacy_fallback: &Option<String>,
) -> Option<String> {
    normalize_device_filter(configured).or_else(|| legacy_fallback.clone())
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
    provider: &mut ProviderTask,
    events: &mpsc::Sender<BleHeartRateEvent>,
    device_filter: &Option<String>,
) {
    match (enabled, provider.is_running()) {
        (true, false) => {
            info!(
                filter = device_filter.as_deref().unwrap_or("<first heart-rate device>"),
                "starting BLE heart-rate provider"
            );
            provider.start(spawn_ble_provider(events.clone(), device_filter.clone()));
        }
        (false, true) => {
            provider.stop();
            info!("BLE heart-rate provider stopped");
        }
        (false, false) => provider.stop(),
        _ => {}
    }
}

fn discard_queued_events(events: &mut mpsc::Receiver<BleHeartRateEvent>) {
    while events.try_recv().is_ok() {}
}

async fn apply_provider_config(
    new_enabled: bool,
    new_configured_filter: Option<String>,
    legacy_filter: &Option<String>,
    enabled: &mut bool,
    device_filter: &mut Option<String>,
    provider: &mut ProviderTask,
    event_tx: &mpsc::Sender<BleHeartRateEvent>,
    event_rx: &mut mpsc::Receiver<BleHeartRateEvent>,
    write_half: &mut tokio::net::unix::OwnedWriteHalf,
    report_disabled_state: bool,
) -> Result<()> {
    let new_filter = effective_device_filter(new_configured_filter, legacy_filter);
    let filter_changed = new_filter != *device_filter;
    let restart = new_enabled && *enabled && filter_changed && provider.is_running();

    if restart {
        provider.stop();
        discard_queued_events(event_rx);
        report_runtime(write_half, false, None).await?;
        info!("BLE heart-rate device selection changed; restarting provider");
    }

    let was_enabled = *enabled;
    *enabled = new_enabled;
    *device_filter = new_filter;
    set_provider_enabled(*enabled, provider, event_tx, device_filter);

    if !*enabled && (report_disabled_state || was_enabled) {
        discard_queued_events(event_rx);
        report_runtime(write_half, false, None).await?;
    }

    Ok(())
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

    let legacy_filter = legacy_device_filter();
    let (event_tx, mut event_rx) = mpsc::channel::<BleHeartRateEvent>(32);
    let mut provider = ProviderTask(None);
    let mut device_filter = legacy_filter.clone();
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
                        apply_provider_config(
                            state.heart_rate.enabled,
                            state.heart_rate.device_filter,
                            &legacy_filter,
                            &mut enabled,
                            &mut device_filter,
                            &mut provider,
                            &event_tx,
                            &mut event_rx,
                            &mut write_half,
                            true,
                        )
                        .await?;
                    }
                    ServerMessage::Event { event: StateEvent::HeartRateChanged(state) } => {
                        let config_changed = state.enabled != enabled
                            || effective_device_filter(state.device_filter.clone(), &legacy_filter)
                                != device_filter;
                        if config_changed {
                            apply_provider_config(
                                state.enabled,
                                state.device_filter,
                                &legacy_filter,
                                &mut enabled,
                                &mut device_filter,
                                &mut provider,
                                &event_tx,
                                &mut event_rx,
                                &mut write_half,
                                false,
                            )
                            .await?;
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

    use super::*;

    #[tokio::test]
    async fn dropping_provider_task_aborts_provider() {
        let completed = Arc::new(AtomicBool::new(false));
        let flag = completed.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, Ordering::Release);
        });

        drop(ProviderTask(Some(task)));
        tokio::time::sleep(Duration::from_millis(80)).await;

        assert!(!completed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn finished_provider_task_is_not_reported_as_running() {
        let task = tokio::spawn(async {});
        let provider = ProviderTask(Some(task));
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(!provider.is_running());
    }

    #[tokio::test]
    async fn disabling_discards_queued_provider_events() {
        let (tx, mut rx) = mpsc::channel(4);
        tx.send(BleHeartRateEvent::Measurement(51)).await.unwrap();
        tx.send(BleHeartRateEvent::Disconnected).await.unwrap();

        discard_queued_events(&mut rx);

        assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
    }

    #[test]
    fn configured_filter_overrides_legacy_environment_fallback() {
        let legacy = Some("legacy strap".into());
        assert_eq!(
            effective_device_filter(Some("  Polar H10  ".into()), &legacy).as_deref(),
            Some("Polar H10")
        );
        assert_eq!(
            effective_device_filter(None, &legacy).as_deref(),
            Some("legacy strap")
        );
    }

    #[test]
    fn empty_configured_filter_falls_back_to_legacy_selection() {
        let legacy = Some("AA:BB:CC".into());
        assert_eq!(
            effective_device_filter(Some("   ".into()), &legacy).as_deref(),
            Some("AA:BB:CC")
        );
    }
}
