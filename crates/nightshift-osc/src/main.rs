use std::{env, path::PathBuf, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use nightshift_protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, SleepPhase, StateEvent,
};
use rosc::{OscMessage, OscPacket, OscType};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UdpSocket, UnixStream},
};
use tracing::{info, warn};

const VRCHAT_OSC_ADDR: &str = "127.0.0.1:9000";
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const SLEEP_PARAMETER: &str = "/avatar/parameters/NightshiftSleeping";

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

async fn osc_sender() -> Result<UdpSocket> {
    let socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .context("binding Nightshift OSC sender")?;
    socket
        .connect(VRCHAT_OSC_ADDR)
        .await
        .with_context(|| format!("connecting OSC sender to {VRCHAT_OSC_ADDR}"))?;
    Ok(socket)
}

async fn send_sleep_state(socket: &UdpSocket, phase: SleepPhase) -> Result<()> {
    let sleeping = matches!(phase, SleepPhase::EnteringSleep | SleepPhase::Sleeping);
    let packet = OscPacket::Message(OscMessage {
        addr: SLEEP_PARAMETER.to_string(),
        args: vec![OscType::Bool(sleeping)],
    });
    let bytes = rosc::encoder::encode(&packet).context("encoding Nightshift OSC packet")?;
    socket
        .send(&bytes)
        .await
        .context("sending Nightshift sleep parameter to VRChat")?;
    Ok(())
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

    let mut enabled = false;
    let mut phase = SleepPhase::Awake;
    let mut sender: Option<UdpSocket> = None;
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
                    send_sleep_state(sender.as_ref().expect("sender created"), phase).await?;
                } else {
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
                        send_sleep_state(sender.as_ref().expect("sender created"), phase).await?;
                    } else {
                        sender = None;
                    }
                }
                StateEvent::SleepChanged(state) => {
                    phase = state.phase;
                    if enabled {
                        if sender.is_none() {
                            sender = Some(osc_sender().await?);
                        }
                        send_sleep_state(sender.as_ref().expect("sender created"), phase).await?;
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
