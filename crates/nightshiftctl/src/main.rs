use std::{env, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use nightshift_protocol::{
    AppState, ClientMessage, OverlaySpawnMode, PROTOCOL_VERSION, ServerMessage, SleepPhase, UiCommand,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

fn usage() -> &'static str {
    "nightshiftctl commands:\n\
       status\n\
       sleep\n\
       wake\n\
       brightness <2-100 percent>\n\
       sleep-brightness <2-100 percent>\n\
       fade <seconds>\n\
       overlay-distance <0.25-5.0 meters>\n\
       overlay-mode <in-front|restore-last-position>\n\n\
       Overlay distance/mode are reserved defaults for the future positioned panel;\n\
       they do not reposition the current full-screen OpenXR dimmer."
}

fn parse_percent(value: Option<String>, command: &str) -> Result<f32> {
    let raw = value.ok_or_else(|| anyhow!("{command} requires a percentage\n\n{}", usage()))?;
    let percent = raw
        .parse::<f32>()
        .with_context(|| format!("invalid percentage {raw:?}"))?;
    if !(2.0..=100.0).contains(&percent) {
        bail!("percentage must be between 2 and 100");
    }
    Ok(percent / 100.0)
}

fn parse_seconds(value: Option<String>) -> Result<f32> {
    let raw = value.ok_or_else(|| anyhow!("fade requires a duration\n\n{}", usage()))?;
    let seconds = raw
        .parse::<f32>()
        .with_context(|| format!("invalid fade duration {raw:?}"))?;
    if !(0.0..=120.0).contains(&seconds) {
        bail!("fade duration must be between 0 and 120 seconds");
    }
    Ok(seconds)
}

fn parse_overlay_distance(value: Option<String>) -> Result<f32> {
    let raw = value.ok_or_else(|| anyhow!("overlay-distance requires meters\n\n{}", usage()))?;
    let meters = raw
        .parse::<f32>()
        .with_context(|| format!("invalid overlay distance {raw:?}"))?;
    if !meters.is_finite() || !(0.25..=5.0).contains(&meters) {
        bail!("overlay distance must be between 0.25 and 5.0 meters");
    }
    Ok(meters)
}

fn parse_overlay_mode(value: Option<String>) -> Result<OverlaySpawnMode> {
    let raw = value.ok_or_else(|| anyhow!("overlay-mode requires a mode\n\n{}", usage()))?;
    match raw.as_str() {
        "in-front" => Ok(OverlaySpawnMode::InFront),
        "restore-last-position" => Ok(OverlaySpawnMode::RestoreLastPosition),
        _ => bail!("overlay mode must be in-front or restore-last-position"),
    }
}

async fn write_message(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: &ClientMessage,
) -> Result<()> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

async fn connect() -> Result<(
    BufReader<tokio::net::unix::OwnedReadHalf>,
    tokio::net::unix::OwnedWriteHalf,
)> {
    let path = socket_path()?;
    let stream = UnixStream::connect(&path)
        .await
        .with_context(|| format!("connecting to Nightshift daemon at {}", path.display()))?;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    write_message(
        &mut write_half,
        &ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            client: "nightshiftctl".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        },
    )
    .await?;

    let mut line = String::new();
    if reader.read_line(&mut line).await? == 0 {
        bail!("daemon closed the socket during handshake");
    }

    match serde_json::from_str::<ServerMessage>(line.trim_end())? {
        ServerMessage::Hello { protocol, .. } if protocol == PROTOCOL_VERSION => {}
        ServerMessage::Hello { protocol, .. } => {
            bail!("protocol mismatch: client={PROTOCOL_VERSION}, daemon={protocol}")
        }
        ServerMessage::Error { message } => bail!("daemon rejected connection: {message}"),
        _ => bail!("daemon returned an unexpected handshake response"),
    }

    Ok((reader, write_half))
}

async fn read_snapshot(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
) -> Result<AppState> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            bail!("daemon disconnected before sending state");
        }
        match serde_json::from_str::<ServerMessage>(line.trim_end())? {
            ServerMessage::FullState { state } => return Ok(state),
            ServerMessage::Error { message } => bail!("daemon error: {message}"),
            ServerMessage::Hello { .. } | ServerMessage::Event { .. } => {}
        }
    }
}

fn phase_name(phase: SleepPhase) -> &'static str {
    match phase {
        SleepPhase::Awake => "awake",
        SleepPhase::EnteringSleep => "entering_sleep",
        SleepPhase::Sleeping => "sleeping",
        SleepPhase::Waking => "waking",
    }
}

fn overlay_mode_name(mode: OverlaySpawnMode) -> &'static str {
    match mode {
        OverlaySpawnMode::InFront => "in-front",
        OverlaySpawnMode::RestoreLastPosition => "restore-last-position",
    }
}

fn print_status(state: &AppState) {
    println!("Nightshift");
    println!("  sleep       : {}", phase_name(state.sleep.phase));
    println!(
        "  brightness  : {:.0}% (target {:.0}%)",
        state.brightness.current * 100.0,
        state.brightness.target * 100.0
    );
    println!("  sleep target: {:.0}%", state.brightness.sleep_target * 100.0);
    println!("  fade        : {:.2}s", state.settings.fade_duration_seconds);
    println!(
        "  dimmer      : {}",
        if state.overlay.runtime_connected {
            "connected"
        } else {
            "disconnected"
        }
    );
    if let Some(version) = state.overlay.runtime_client_version.as_deref() {
        println!("  dimmer ver  : {version}");
    }
    println!(
        "  panel dist  : {:.2}m (reserved)",
        state.overlay.spawn_distance_m
    );
    println!(
        "  panel mode  : {} (reserved)",
        overlay_mode_name(state.overlay.spawn_mode)
    );
    println!(
        "  vrchat      : {}",
        if state.vrchat.connected { "connected" } else { "disconnected" }
    );

    let osc_status = if !state.osc.enabled {
        "off"
    } else if state.osc.connected {
        "running"
    } else {
        "enabled, worker missing"
    };
    println!("  osc         : {osc_status}");
    if state.osc.connected {
        println!(
            "  osc avatar  : {}",
            state.osc.avatar_id.as_deref().unwrap_or("not discovered")
        );
        println!("  osc params  : {}", state.osc.avatar_parameter_count);
        println!(
            "  gogo 1.8+   : {}",
            if state.osc.gogo_detected { "detected" } else { "not detected" }
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "status".into());

    let action = match command.as_str() {
        "status" => None,
        "sleep" => Some(UiCommand::EnterSleep),
        "wake" => Some(UiCommand::Wake),
        "brightness" => Some(UiCommand::SetBrightness(parse_percent(args.next(), "brightness")?)),
        "sleep-brightness" => Some(UiCommand::SetSleepBrightness(parse_percent(
            args.next(),
            "sleep-brightness",
        )?)),
        "fade" => Some(UiCommand::SetFadeDuration(parse_seconds(args.next())?)),
        "overlay-distance" => Some(UiCommand::SetOverlaySpawnDistance(parse_overlay_distance(
            args.next(),
        )?)),
        "overlay-mode" => Some(UiCommand::SetOverlaySpawnMode(parse_overlay_mode(args.next())?)),
        "help" | "--help" | "-h" => {
            println!("{}", usage());
            return Ok(());
        }
        other => bail!("unknown command {other:?}\n\n{}", usage()),
    };

    if args.next().is_some() {
        bail!("too many arguments\n\n{}", usage());
    }

    let (mut reader, mut writer) = connect().await?;
    let state = read_snapshot(&mut reader).await?;

    if let Some(command) = action {
        write_message(&mut writer, &ClientMessage::Command { command }).await?;
        writer.shutdown().await?;
        println!("ok");
    } else {
        print_status(&state);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_overlay_distance_bounds() {
        assert_eq!(parse_overlay_distance(Some("0.25".into())).unwrap(), 0.25);
        assert_eq!(parse_overlay_distance(Some("5.0".into())).unwrap(), 5.0);
        assert!(parse_overlay_distance(Some("0.24".into())).is_err());
        assert!(parse_overlay_distance(Some("5.01".into())).is_err());
        assert!(parse_overlay_distance(Some("NaN".into())).is_err());
    }

    #[test]
    fn parses_overlay_modes() {
        assert_eq!(
            parse_overlay_mode(Some("in-front".into())).unwrap(),
            OverlaySpawnMode::InFront
        );
        assert_eq!(
            parse_overlay_mode(Some("restore-last-position".into())).unwrap(),
            OverlaySpawnMode::RestoreLastPosition
        );
        assert!(parse_overlay_mode(Some("dimmer".into())).is_err());
    }
}
