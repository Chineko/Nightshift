use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use nightshift_protocol::{
    AppState, ClientMessage, PROTOCOL_VERSION, ServerMessage, StateEvent, UiCommand, WorkerReport,
};

use crate::panel_state::PanelStateTracker;
use crate::runtime_state::{
    DEFAULT_TRANSITION_SECONDS, RuntimeUpdate, brightness_target, sanitize_transition_seconds,
};
use crate::surface::SurfaceTarget;

const RECONNECT_DELAY: Duration = Duration::from_secs(1);

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

pub(crate) fn hidden_surface_target() -> SurfaceTarget {
    SurfaceTarget::from(&nightshift_protocol::OverlayState::default())
}

pub(crate) fn spawn_ipc_client(
    running: Arc<AtomicBool>,
    updates: Sender<RuntimeUpdate>,
    reports: Receiver<WorkerReport>,
    commands: Receiver<UiCommand>,
    hydra_mode: bool,
) {
    thread::spawn(move || {
        let path = match socket_path() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("Nightshift overlay IPC disabled: {error:#}");
                return;
            }
        };

        let mut announced_waiting = false;

        while running.load(Ordering::Relaxed) {
            match UnixStream::connect(&path) {
                Ok(stream) => {
                    announced_waiting = false;
                    println!("Nightshift overlay connected to daemon IPC.");
                    if let Err(error) = run_ipc_connection(
                        stream, &updates, &reports, &commands, &running, hydra_mode,
                    ) {
                        if running.load(Ordering::Relaxed) {
                            eprintln!("Nightshift overlay IPC disconnected: {error:#}");
                        }
                    }
                    while commands.try_recv().is_ok() {}
                    let _ = updates.send(RuntimeUpdate::Surface(hidden_surface_target()));
                }
                Err(error) => {
                    if !announced_waiting {
                        eprintln!(
                            "Nightshift overlay waiting for daemon at {} ({error})",
                            path.display()
                        );
                        announced_waiting = true;
                    }
                }
            }

            if running.load(Ordering::Relaxed) {
                thread::sleep(RECONNECT_DELAY);
            }
        }
    });
}

fn write_client_message(writer: &mut UnixStream, message: &ClientMessage) -> Result<()> {
    let mut json = serde_json::to_vec(message).context("serializing daemon IPC message")?;
    json.push(b'\n');
    writer
        .write_all(&json)
        .context("sending daemon IPC message")?;
    writer.flush().context("flushing daemon IPC message")?;
    Ok(())
}

fn flush_client_messages(
    writer: &mut UnixStream,
    reports: &Receiver<WorkerReport>,
    commands: &Receiver<UiCommand>,
    hydra_mode: bool,
) -> Result<()> {
    while let Ok(report) = reports.try_recv() {
        if hydra_mode {
            println!("[Hydra/Pose] report={report:?}");
        }
        write_client_message(writer, &ClientMessage::Report { report })?;
    }
    while let Ok(command) = commands.try_recv() {
        if hydra_mode {
            println!("[Hydra/Input] daemon command={command:?}");
        }
        write_client_message(writer, &ClientMessage::Command { command })?;
    }
    Ok(())
}

fn run_ipc_connection(
    stream: UnixStream,
    updates: &Sender<RuntimeUpdate>,
    reports: &Receiver<WorkerReport>,
    commands: &Receiver<UiCommand>,
    running: &AtomicBool,
    hydra_mode: bool,
) -> Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .context("setting daemon IPC read timeout")?;

    let mut writer = stream.try_clone().context("cloning daemon IPC socket")?;
    write_client_message(
        &mut writer,
        &ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            client: "nightshift-overlay".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        },
    )?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut fade_seconds = DEFAULT_TRANSITION_SECONDS;
    let mut panel_state = PanelStateTracker::default();

    while running.load(Ordering::Relaxed) {
        flush_client_messages(&mut writer, reports, commands, hydra_mode)?;
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => bail!("daemon closed the socket"),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error).context("reading daemon IPC"),
        }

        let message: ServerMessage =
            serde_json::from_str(line.trim_end()).context("decoding daemon IPC message")?;

        match message {
            ServerMessage::Hello { protocol, .. } => {
                if protocol != PROTOCOL_VERSION {
                    bail!(
                        "daemon protocol mismatch: overlay={}, daemon={protocol}",
                        PROTOCOL_VERSION
                    );
                }
                if hydra_mode {
                    println!("[Hydra/IPC] handshake=accepted protocol={protocol}");
                }
            }
            ServerMessage::FullState { state } => {
                fade_seconds = sanitize_transition_seconds(state.settings.fade_duration_seconds);
                panel_state = PanelStateTracker::from_state(state.clone());
                send_state_targets(updates, &state, fade_seconds)?;
                updates
                    .send(RuntimeUpdate::Panel(panel_state.content().clone()))
                    .map_err(|_| anyhow!("overlay renderer stopped"))?;
                if hydra_mode {
                    println!(
                        "[Hydra/IPC] snapshot target={:.2}% fade={:.2}s surface_visible={}",
                        state.brightness.target * 100.0,
                        fade_seconds,
                        state.overlay.visible
                    );
                }
            }
            ServerMessage::Event { event } => {
                if let Some(content) = panel_state.apply_event(&event) {
                    updates
                        .send(RuntimeUpdate::Panel(content))
                        .map_err(|_| anyhow!("overlay renderer stopped"))?;
                }

                match event {
                    StateEvent::BrightnessChanged(brightness) => {
                        let target = brightness_target(&brightness, fade_seconds);
                        updates
                            .send(RuntimeUpdate::Brightness(target))
                            .map_err(|_| anyhow!("overlay renderer stopped"))?;

                        if hydra_mode {
                            println!(
                                "[Hydra/IPC] brightness target={:.2}% transition={:.2}s daemon_transitioning={}",
                                target.brightness * 100.0,
                                target.transition_seconds,
                                brightness.transitioning
                            );
                        }
                    }
                    StateEvent::OverlayChanged(overlay) => {
                        let target = SurfaceTarget::from(&overlay);
                        updates
                            .send(RuntimeUpdate::Surface(target))
                            .map_err(|_| anyhow!("overlay renderer stopped"))?;
                        if hydra_mode {
                            println!(
                                "[Hydra/IPC] surface visible={} distance={:.2}m mode={:?}",
                                target.visible, target.distance_m, target.spawn_mode
                            );
                        }
                    }
                    StateEvent::SettingsChanged(settings) => {
                        fade_seconds = sanitize_transition_seconds(settings.fade_duration_seconds);
                        if hydra_mode {
                            println!("[Hydra/IPC] fade default={fade_seconds:.2}s");
                        }
                    }
                    _ => {}
                }
            }
            ServerMessage::Error { message } => bail!("daemon rejected overlay: {message}"),
        }
    }

    Ok(())
}

fn send_state_targets(
    updates: &Sender<RuntimeUpdate>,
    state: &AppState,
    fade_seconds: f32,
) -> Result<()> {
    updates
        .send(RuntimeUpdate::Brightness(brightness_target(
            &state.brightness,
            fade_seconds,
        )))
        .map_err(|_| anyhow!("overlay renderer stopped"))?;
    updates
        .send(RuntimeUpdate::Surface(SurfaceTarget::from(&state.overlay)))
        .map_err(|_| anyhow!("overlay renderer stopped"))
}
