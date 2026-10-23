use std::{env, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use nightshift_protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, SleepPhase, StateEvent,
};
use rosc::{OscMessage, OscPacket, OscType};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UdpSocket, UnixStream},
    task::JoinHandle,
};
use tracing::{info, warn};

const VRCHAT_OSC_ADDR: &str = "127.0.0.1:9000";
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const SLEEP_PARAMETER: &str = "/avatar/parameters/NightshiftSleeping";

#[derive(Clone, Debug, Deserialize)]
struct OscAutomationConfig {
    #[serde(default = "default_true")]
    publish_sleep_parameter: bool,
    #[serde(default)]
    on_sleep: Vec<OscStep>,
    #[serde(default)]
    on_wake: Vec<OscStep>,
}

impl Default for OscAutomationConfig {
    fn default() -> Self {
        Self {
            publish_sleep_parameter: true,
            on_sleep: Vec::new(),
            on_wake: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OscStep {
    Send { address: String, value: OscValue },
    Delay { milliseconds: u64 },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum OscValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    String(String),
}

impl From<OscValue> for OscType {
    fn from(value: OscValue) -> Self {
        match value {
            OscValue::Bool(value) => Self::Bool(value),
            OscValue::Int(value) => Self::Int(value),
            OscValue::Float(value) => Self::Float(value),
            OscValue::String(value) => Self::String(value),
        }
    }
}

fn default_true() -> bool {
    true
}

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

fn config_path() -> Result<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .context("neither XDG_CONFIG_HOME nor HOME is set")?;
    Ok(base.join("nightshift").join("osc.json"))
}

async fn load_config() -> Result<OscAutomationConfig> {
    let path = config_path()?;
    match tokio::fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing OSC automation config {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            info!(path = %path.display(), "OSC automation config not found; using defaults");
            Ok(OscAutomationConfig::default())
        }
        Err(error) => Err(error)
            .with_context(|| format!("reading OSC automation config {}", path.display())),
    }
}

async fn osc_sender() -> Result<Arc<UdpSocket>> {
    let socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .context("binding Nightshift OSC sender")?;
    socket
        .connect(VRCHAT_OSC_ADDR)
        .await
        .with_context(|| format!("connecting OSC sender to {VRCHAT_OSC_ADDR}"))?;
    Ok(Arc::new(socket))
}

async fn send_value(socket: &UdpSocket, address: &str, value: OscValue) -> Result<()> {
    let packet = OscPacket::Message(OscMessage {
        addr: address.to_string(),
        args: vec![value.into()],
    });
    let bytes = rosc::encoder::encode(&packet).context("encoding OSC packet")?;
    socket
        .send(&bytes)
        .await
        .with_context(|| format!("sending OSC packet to {address}"))?;
    Ok(())
}

async fn send_sleep_parameter(socket: &UdpSocket, sleeping: bool) -> Result<()> {
    send_value(socket, SLEEP_PARAMETER, OscValue::Bool(sleeping)).await
}

async fn execute_sequence(socket: Arc<UdpSocket>, steps: Vec<OscStep>) -> Result<()> {
    for step in steps {
        match step {
            OscStep::Send { address, value } => {
                send_value(&socket, &address, value).await?;
            }
            OscStep::Delay { milliseconds } => {
                tokio::time::sleep(Duration::from_millis(milliseconds)).await;
            }
        }
    }
    Ok(())
}

fn sleeping_phase(phase: SleepPhase) -> bool {
    matches!(phase, SleepPhase::EnteringSleep | SleepPhase::Sleeping)
}

fn start_automation(
    active: &mut Option<JoinHandle<()>>,
    socket: Arc<UdpSocket>,
    config: &OscAutomationConfig,
    phase: SleepPhase,
) {
    if let Some(task) = active.take() {
        task.abort();
    }

    let sleeping = sleeping_phase(phase);
    let publish_sleep_parameter = config.publish_sleep_parameter;
    let steps = if sleeping {
        config.on_sleep.clone()
    } else {
        config.on_wake.clone()
    };

    *active = Some(tokio::spawn(async move {
        if publish_sleep_parameter {
            if let Err(error) = send_sleep_parameter(&socket, sleeping).await {
                warn!(%error, "failed to publish NightshiftSleeping OSC parameter");
            }
        }

        if let Err(error) = execute_sequence(socket, steps).await {
            warn!(%error, "OSC automation sequence failed");
        }
    }));
}

async fn run_connection(stream: UnixStream) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let hello = ClientMessage::Hello {
        protocol: PROTOCOL_VERSION,
        client: "nightshift-osc".into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
    };
    let mut bytes = serde_json::to_vec(&hello).context("serializing daemon hello")?;
    bytes.push(b'\n');
    write_half
        .write_all(&bytes)
        .await
        .context("sending daemon hello")?;

    let config = load_config().await?;
    let mut enabled = false;
    let mut phase = SleepPhase::Awake;
    let mut sender: Option<Arc<UdpSocket>> = None;
    let mut active_automation: Option<JoinHandle<()>> = None;
    let mut line = String::new();

    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .await
            .context("reading daemon IPC")?
            == 0
        {
            bail!("daemon closed the socket");
        }

        let message: ServerMessage =
            serde_json::from_str(line.trim_end()).context("decoding daemon IPC message")?;

        match message {
            ServerMessage::Hello { protocol, .. } => {
                if protocol != PROTOCOL_VERSION {
                    bail!(
                        "daemon protocol mismatch: osc={}, daemon={protocol}",
                        PROTOCOL_VERSION
                    );
                }
                info!(protocol, "Nightshift OSC worker connected to daemon");
            }
            ServerMessage::FullState { state } => {
                enabled = state.osc.enabled;
                phase = state.sleep.phase;
                if enabled {
                    if sender.is_none() {
                        sender = Some(osc_sender().await?);
                    }
                    start_automation(
                        &mut active_automation,
                        sender.as_ref().expect("sender created").clone(),
                        &config,
                        phase,
                    );
                } else {
                    if let Some(task) = active_automation.take() {
                        task.abort();
                    }
                    sender = None;
                }
            }
            ServerMessage::Event { event } => match event {
                StateEvent::OscChanged(state) => {
                    enabled = state.enabled;
                    if enabled {
                        if sender.is_none() {
                            sender = Some(osc_sender().await?);
                        }
                        start_automation(
                            &mut active_automation,
                            sender.as_ref().expect("sender created").clone(),
                            &config,
                            phase,
                        );
                    } else {
                        if let Some(task) = active_automation.take() {
                            task.abort();
                        }
                        sender = None;
                    }
                }
                StateEvent::SleepChanged(state) => {
                    phase = state.phase;
                    if enabled {
                        if sender.is_none() {
                            sender = Some(osc_sender().await?);
                        }
                        start_automation(
                            &mut active_automation,
                            sender.as_ref().expect("sender created").clone(),
                            &config,
                            phase,
                        );
                    }
                }
                _ => {}
            },
            ServerMessage::Error { message } => bail!("daemon rejected OSC worker: {message}"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nightshift_osc=info".into()),
        )
        .init();

    let path = socket_path()?;

    loop {
        match UnixStream::connect(&path).await {
            Ok(stream) => {
                if let Err(error) = run_connection(stream).await {
                    warn!(%error, "Nightshift OSC worker disconnected");
                }
            }
            Err(error) => {
                warn!(%error, path = %path.display(), "waiting for Nightshift daemon");
            }
        }

        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_generic_sleep_sequence() {
        let json = r#"
        {
          "publish_sleep_parameter": true,
          "on_sleep": [
            {
              "type": "send",
              "address": "/avatar/parameters/Go/FeetLock",
              "value": { "type": "bool", "value": true }
            },
            { "type": "delay", "milliseconds": 150 },
            {
              "type": "send",
              "address": "/avatar/parameters/Go/Pose",
              "value": { "type": "int", "value": 2 }
            }
          ],
          "on_wake": []
        }
        "#;

        let config: OscAutomationConfig = serde_json::from_str(json).unwrap();
        assert!(config.publish_sleep_parameter);
        assert_eq!(config.on_sleep.len(), 3);
        assert!(config.on_wake.is_empty());
    }
}
