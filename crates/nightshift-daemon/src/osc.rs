use std::time::Duration;

use anyhow::{Context, Result};
use nightshift_protocol::{SleepPhase, StateEvent};
use rosc::{OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;
use tracing::{info, warn};

use crate::StateStore;

const VRCHAT_OSC_ADDR: &str = "127.0.0.1:9000";
const NIGHTSHIFT_OSC_ADDR: &str = "127.0.0.1:9001";
const REBIND_DELAY: Duration = Duration::from_secs(2);
const SLEEP_PARAMETER: &str = "/avatar/parameters/NightshiftSleeping";

pub(crate) async fn run(store: StateStore) {
    loop {
        if let Err(error) = run_bound(store.clone()).await {
            store.set_osc_connected(false).await;
            warn!(%error, "Nightshift OSC service stopped; retrying");
            tokio::time::sleep(REBIND_DELAY).await;
        }
    }
}

async fn run_bound(store: StateStore) -> Result<()> {
    let socket = UdpSocket::bind(NIGHTSHIFT_OSC_ADDR)
        .await
        .with_context(|| format!("binding OSC receiver at {NIGHTSHIFT_OSC_ADDR}"))?;
    socket
        .connect(VRCHAT_OSC_ADDR)
        .await
        .with_context(|| format!("connecting OSC sender to {VRCHAT_OSC_ADDR}"))?;

    info!(
        receive = NIGHTSHIFT_OSC_ADDR,
        send = VRCHAT_OSC_ADDR,
        "Nightshift OSC socket ready"
    );

    let (snapshot, mut updates) = store.snapshot_and_subscribe().await;
    let mut enabled = snapshot.osc.enabled;
    store.set_osc_connected(enabled).await;

    if enabled {
        send_sleep_state(&socket, snapshot.sleep.phase).await?;
    }

    let mut recv_buffer = [0_u8; 65_535];

    loop {
        tokio::select! {
            received = socket.recv(&mut recv_buffer) => {
                let size = received.context("receiving VRChat OSC packet")?;
                // Decoding validates that this is OSC traffic. Nightshift does not need to
                // consume the payload yet, but keeping the receive side alive makes this
                // service ready for avatar-change and parameter inputs later.
                if let Err(error) = rosc::decoder::decode_udp(&recv_buffer[..size]) {
                    warn!(%error, "ignoring malformed OSC packet");
                }
            }
            event = updates.recv() => {
                match event {
                    Ok(StateEvent::OscChanged(state)) => {
                        enabled = state.enabled;
                        store.set_osc_connected(enabled).await;
                        if enabled {
                            send_sleep_state(&socket, store.sleep_phase().await).await?;
                        }
                    }
                    Ok(StateEvent::SleepChanged(state)) if enabled => {
                        send_sleep_state(&socket, state.phase).await?;
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let (snapshot, new_updates) = store.snapshot_and_subscribe().await;
                        updates = new_updates;
                        enabled = snapshot.osc.enabled;
                        store.set_osc_connected(enabled).await;
                        if enabled {
                            send_sleep_state(&socket, snapshot.sleep.phase).await?;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
        }
    }
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
