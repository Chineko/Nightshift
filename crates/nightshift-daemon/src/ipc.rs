use super::*;

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

pub(crate) fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

pub(crate) async fn send_message(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: &ServerMessage,
) -> Result<()> {
    let mut json = serde_json::to_vec(message)?;
    json.push(b'\n');
    writer.write_all(&json).await?;
    Ok(())
}

pub(crate) async fn read_ipc_line<R>(reader: &mut R, buffer: &mut Vec<u8>) -> Result<Option<String>>
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

pub(crate) async fn handle_client(stream: UnixStream, store: StateStore) -> Result<()> {
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
