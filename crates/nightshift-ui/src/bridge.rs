use super::*;

pub(crate) enum UiUpdate {
    Connected(bool),
    Message(ServerMessage),
}

pub(crate) struct Bridge {
    pub(crate) commands: tokio_mpsc::UnboundedSender<UiCommand>,
    pub(crate) updates: mpsc::Receiver<UiUpdate>,
}

impl Bridge {
    pub(crate) fn spawn(ctx: egui::Context) -> Self {
        let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
        let (update_tx, update_rx) = mpsc::channel();

        thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("failed to create Nightshift UI runtime");
            runtime.block_on(connection_loop(ctx, command_rx, update_tx));
        });

        Self {
            commands: command_tx,
            updates: update_rx,
        }
    }

    pub(crate) fn send(&self, command: UiCommand) {
        let _ = self.commands.send(command);
    }
}

fn socket_path() -> Option<PathBuf> {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|path| path.join("nightshift").join("nightshift.sock"))
}

fn discard_queued_commands(commands: &mut tokio_mpsc::UnboundedReceiver<UiCommand>) -> bool {
    loop {
        match commands.try_recv() {
            Ok(_) => {}
            Err(tokio_mpsc::error::TryRecvError::Empty) => return true,
            Err(tokio_mpsc::error::TryRecvError::Disconnected) => return false,
        }
    }
}

async fn connection_loop(
    ctx: egui::Context,
    mut commands: tokio_mpsc::UnboundedReceiver<UiCommand>,
    updates: mpsc::Sender<UiUpdate>,
) {
    loop {
        if !discard_queued_commands(&mut commands) {
            return;
        }

        let Some(path) = socket_path() else {
            let _ = updates.send(UiUpdate::Connected(false));
            ctx.request_repaint();
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };

        let stream = match UnixStream::connect(&path).await {
            Ok(stream) => stream,
            Err(_) => {
                let _ = updates.send(UiUpdate::Connected(false));
                ctx.request_repaint();
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let hello = ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            client: "nightshift-ui".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        };

        if write_json_line(&mut write_half, &hello).await.is_err() {
            continue;
        }

        let mut line = String::new();
        match tokio::time::timeout(
            nightshift_protocol::IPC_HELLO_TIMEOUT,
            reader.read_line(&mut line),
        )
        .await
        {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => continue,
            Ok(Ok(_)) => match serde_json::from_str::<ServerMessage>(line.trim_end()) {
                Ok(ServerMessage::Hello { protocol, .. }) if protocol == PROTOCOL_VERSION => {}
                Ok(ServerMessage::Error { .. }) | Ok(_) | Err(_) => continue,
            },
        }

        if updates.send(UiUpdate::Connected(true)).is_err() {
            return;
        }
        ctx.request_repaint();

        loop {
            line.clear();
            tokio::select! {
                read = reader.read_line(&mut line) => {
                    match read {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if let Ok(message) = serde_json::from_str::<ServerMessage>(line.trim_end()) {
                                if updates.send(UiUpdate::Message(message)).is_err() {
                                    return;
                                }
                                ctx.request_repaint();
                            }
                        }
                    }
                }
                command = commands.recv() => {
                    let Some(command) = command else { return; };
                    let message = ClientMessage::Command { command };
                    if write_json_line(&mut write_half, &message).await.is_err() {
                        break;
                    }
                }
            }
        }

        let _ = updates.send(UiUpdate::Connected(false));
        ctx.request_repaint();
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn write_json_line(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: &ClientMessage,
) -> Result<(), serde_json::Error> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    if writer.write_all(&bytes).await.is_err() {
        return Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "Nightshift IPC disconnected",
        )));
    }
    Ok(())
}
