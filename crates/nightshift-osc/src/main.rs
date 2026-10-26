mod avatar_context;
mod gogo;
mod resolver;

use std::{env, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use avatar_context::{AvatarContext, discover_avatar_context};
use nightshift_osc_script::{
    CURRENT_SCRIPT_VERSION, OscAction, OscScript, OscValue, ScriptTaskRegistry,
};
use nightshift_protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, SleepPhase, SleepingPose, StateEvent,
    WorkerReport,
};
use resolver::{ResolveResult, SemanticQuery, resolve_parameter};
use rosc::{OscMessage, OscPacket, OscType};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UdpSocket, UnixStream},
};
use tracing::{info, warn};

const VRCHAT_OSC_ADDR: &str = "127.0.0.1:9000";
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const SLEEP_PARAMETER: &str = "/avatar/parameters/NightshiftSleeping";
const SLEEP_TRANSITION_GROUP: &str = "sleep_transition";
const SLEEP_POSE_GROUP: &str = "sleep_pose";

#[derive(Clone, Debug, Deserialize)]
struct OscAutomationConfig {
    #[serde(default = "default_true")]
    publish_sleep_parameter: bool,
    #[serde(default)]
    on_sleep: ConfigScript,
    #[serde(default)]
    on_wake: ConfigScript,
}

impl Default for OscAutomationConfig {
    fn default() -> Self {
        Self {
            publish_sleep_parameter: true,
            on_sleep: ConfigScript::default(),
            on_wake: ConfigScript::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum ConfigScript {
    Versioned(OscScript),
    Legacy(Vec<OscAction>),
}

impl Default for ConfigScript {
    fn default() -> Self {
        Self::Versioned(OscScript::default())
    }
}

impl ConfigScript {
    fn to_script(&self) -> OscScript {
        match self {
            Self::Versioned(script) => script.clone(),
            Self::Legacy(actions) => OscScript {
                version: CURRENT_SCRIPT_VERSION,
                actions: actions.clone(),
            },
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

fn osc_type(value: OscValue) -> OscType {
    match value {
        OscValue::Bool(value) => OscType::Bool(value),
        OscValue::Int(value) => OscType::Int(value),
        OscValue::Float(value) => OscType::Float(value),
        OscValue::String(value) => OscType::String(value),
    }
}

async fn send_value(socket: &UdpSocket, address: &str, value: OscValue) -> Result<()> {
    let packet = OscPacket::Message(OscMessage {
        addr: address.to_string(),
        args: vec![osc_type(value)],
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

async fn execute_script(
    socket: Arc<UdpSocket>,
    script: OscScript,
    avatar_context: Option<AvatarContext>,
) -> Result<()> {
    if script.version != CURRENT_SCRIPT_VERSION {
        bail!(
            "unsupported OSC script version {}; expected {}",
            script.version,
            CURRENT_SCRIPT_VERSION
        );
    }

    for action in script.actions {
        match action {
            OscAction::Send { address, value } => {
                let targets = avatar_context
                    .as_ref()
                    .map(|context| context.target_addresses_for(&address))
                    .unwrap_or_else(|| vec![address.as_str()]);
                for target in targets {
                    send_value(&socket, target, value.clone()).await?;
                }
            }
            OscAction::Delay { milliseconds } => {
                tokio::time::sleep(Duration::from_millis(milliseconds)).await;
            }
        }
    }
    Ok(())
}

fn sleeping_phase(phase: SleepPhase) -> bool {
    matches!(phase, SleepPhase::EnteringSleep | SleepPhase::Sleeping)
}

fn transition_edge(previous_sleeping: bool, phase: SleepPhase) -> Option<bool> {
    let sleeping = sleeping_phase(phase);
    (sleeping != previous_sleeping).then_some(sleeping)
}

async fn sync_sleep_parameter(
    socket: &UdpSocket,
    config: &OscAutomationConfig,
    sleeping: bool,
) {
    if config.publish_sleep_parameter {
        if let Err(error) = send_sleep_parameter(socket, sleeping).await {
            warn!(%error, "failed to synchronize NightshiftSleeping OSC parameter");
        }
    }
}

async fn refresh_avatar_context(current: &mut Option<AvatarContext>) {
    match discover_avatar_context().await {
        Ok(Some(context)) => {
            let changed = current.as_ref() != Some(&context);
            if changed {
                info!(
                    avatar_id = %context.avatar_id,
                    parameters = context.parameters.len(),
                    "discovered VRChat avatar OSC context"
                );
            }
            *current = Some(context);
        }
        Ok(None) => {
            if current.take().is_some() {
                info!("VRChat OSCQuery context is no longer available");
            }
        }
        Err(error) => {
            warn!(%error, "failed to refresh VRChat OSCQuery avatar context");
        }
    }
}

fn runtime_report(avatar_context: Option<&AvatarContext>) -> WorkerReport {
    WorkerReport::OscRuntime {
        connected: true,
        avatar_id: avatar_context.map(|context| context.avatar_id.clone()),
        avatar_parameter_count: avatar_context
            .map(|context| u32::try_from(context.parameters.len()).unwrap_or(u32::MAX))
            .unwrap_or(0),
        gogo_detected: avatar_context.is_some_and(gogo::supports_gogo_1_8),
    }
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

async fn report_runtime_if_changed(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    previous: &mut Option<WorkerReport>,
    avatar_context: Option<&AvatarContext>,
) -> Result<()> {
    let report = runtime_report(avatar_context);
    if previous.as_ref() == Some(&report) {
        return Ok(());
    }

    write_client_message(
        writer,
        &ClientMessage::Report {
            report: report.clone(),
        },
    )
    .await?;
    *previous = Some(report);
    Ok(())
}

fn custom_transition_script(config: &OscAutomationConfig, sleeping: bool) -> OscScript {
    if sleeping {
        config.on_sleep.to_script()
    } else {
        config.on_wake.to_script()
    }
}

fn start_transition_automation(
    scripts: &mut ScriptTaskRegistry,
    socket: Arc<UdpSocket>,
    config: &OscAutomationConfig,
    sleeping: bool,
    gogo_enabled: bool,
    avatar_context: Option<AvatarContext>,
) {
    let publish_sleep_parameter = config.publish_sleep_parameter;
    let custom_script = custom_transition_script(config, sleeping);

    let gogo_script = if gogo_enabled {
        match avatar_context.as_ref() {
            Some(context) => {
                let script = gogo::sleep_transition_script(context, sleeping);
                if script.is_none() {
                    warn!(
                        avatar_id = %context.avatar_id,
                        "GoGo automation enabled, but the current avatar does not expose a complete GoGo Loco 1.8+ parameter set"
                    );
                }
                script
            }
            None => {
                warn!("GoGo automation enabled, but no VRChat OSCQuery avatar context is available");
                None
            }
        }
    } else {
        None
    };

    scripts.replace(
        SLEEP_TRANSITION_GROUP,
        tokio::spawn(async move {
            if publish_sleep_parameter {
                if let Err(error) = send_sleep_parameter(&socket, sleeping).await {
                    warn!(%error, "failed to publish NightshiftSleeping OSC parameter");
                }
            }

            if let Some(script) = gogo_script {
                if let Err(error) = execute_script(socket.clone(), script, avatar_context.clone()).await {
                    warn!(%error, "built-in GoGo sleep transition script failed");
                }
            }

            if !custom_script.actions.is_empty() {
                if let Err(error) = execute_script(socket, custom_script, avatar_context).await {
                    warn!(%error, "custom sleep transition OSC script failed");
                }
            }
        }),
    );
}

fn start_pose_automation(
    scripts: &mut ScriptTaskRegistry,
    socket: Arc<UdpSocket>,
    pose: SleepingPose,
    avatar_context: Option<AvatarContext>,
) {
    let Some(context) = avatar_context else {
        return;
    };
    let Some(script) = gogo::pose_change_script(&context, pose) else {
        return;
    };

    scripts.replace(
        SLEEP_POSE_GROUP,
        tokio::spawn(async move {
            if let Err(error) = execute_script(socket, script, Some(context)).await {
                warn!(%error, "GoGo sleeping-pose automation failed");
            }
        }),
    );
}

fn log_example_semantic_resolution() {
    let empty: [&str; 0] = [];
    let result = resolve_parameter(
        empty,
        &SemanticQuery {
            required: &["feet", "lock"],
            preferred: &["gogo", "locomotion"],
        },
    );
    if !matches!(result, ResolveResult::Missing) {
        warn!("unexpected semantic resolver result without discovered parameters");
    }
}

async fn run_connection(stream: UnixStream) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    write_client_message(
        &mut write_half,
        &ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            client: "nightshift-osc".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        },
    )
    .await?;

    let mut line = String::new();
    let read = tokio::time::timeout(
        nightshift_protocol::IPC_HELLO_TIMEOUT,
        reader.read_line(&mut line),
    )
    .await
    .context("timed out waiting for daemon hello")??;
    if read == 0 {
        bail!("daemon closed the socket during handshake");
    }
    match serde_json::from_str::<ServerMessage>(line.trim_end())
        .context("decoding daemon hello")?
    {
        ServerMessage::Hello { protocol, .. } if protocol == PROTOCOL_VERSION => {
            info!(protocol, "Nightshift OSC worker connected to daemon");
        }
        ServerMessage::Hello { protocol, .. } => bail!(
            "daemon protocol mismatch: osc={}, daemon={protocol}",
            PROTOCOL_VERSION
        ),
        ServerMessage::Error { message } => bail!("daemon rejected OSC worker: {message}"),
        _ => bail!("daemon returned an unexpected handshake response"),
    }

    let config = load_config().await?;
    let mut enabled = false;
    let mut gogo_enabled = false;
    let mut sleeping = false;
    let mut pose = SleepingPose::Unknown;
    let mut sender: Option<Arc<UdpSocket>> = None;
    let mut avatar_context: Option<AvatarContext> = None;
    let mut last_report: Option<WorkerReport> = None;
    let mut scripts = ScriptTaskRegistry::default();

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
            ServerMessage::Hello { .. } => bail!("daemon repeated IPC hello after handshake"),
            ServerMessage::FullState { state } => {
                enabled = state.osc.enabled;
                gogo_enabled = state.settings.gogo_automation_enabled;
                sleeping = sleeping_phase(state.sleep.phase);
                pose = state.sleep.pose;

                scripts.abort_all();
                if enabled {
                    if sender.is_none() {
                        sender = Some(osc_sender().await?);
                    }
                    sync_sleep_parameter(
                        sender.as_ref().expect("sender created"),
                        &config,
                        sleeping,
                    )
                    .await;
                    refresh_avatar_context(&mut avatar_context).await;
                } else {
                    sender = None;
                    avatar_context = None;
                }
                report_runtime_if_changed(
                    &mut write_half,
                    &mut last_report,
                    avatar_context.as_ref(),
                )
                .await?;
            }
            ServerMessage::Event { event } => match event {
                StateEvent::OscChanged(state) => {
                    enabled = state.enabled;
                    scripts.abort_all();
                    if enabled {
                        if sender.is_none() {
                            sender = Some(osc_sender().await?);
                        }
                        sync_sleep_parameter(
                            sender.as_ref().expect("sender created"),
                            &config,
                            sleeping,
                        )
                        .await;
                        refresh_avatar_context(&mut avatar_context).await;
                    } else {
                        sender = None;
                        avatar_context = None;
                    }
                    report_runtime_if_changed(
                        &mut write_half,
                        &mut last_report,
                        avatar_context.as_ref(),
                    )
                    .await?;
                }
                StateEvent::SettingsChanged(settings) => {
                    let was_enabled = gogo_enabled;
                    gogo_enabled = settings.gogo_automation_enabled;
                    if was_enabled && !gogo_enabled {
                        scripts.abort(SLEEP_POSE_GROUP);
                    } else if enabled && !was_enabled && gogo_enabled {
                        refresh_avatar_context(&mut avatar_context).await;
                    }
                    report_runtime_if_changed(
                        &mut write_half,
                        &mut last_report,
                        avatar_context.as_ref(),
                    )
                    .await?;
                }
                StateEvent::SleepChanged(state) => {
                    let edge = transition_edge(sleeping, state.phase);
                    let pose_changed = pose != state.pose;
                    sleeping = sleeping_phase(state.phase);
                    pose = state.pose;

                    if enabled {
                        if let Some(new_sleeping) = edge {
                            if !new_sleeping {
                                scripts.abort(SLEEP_POSE_GROUP);
                            }
                            if sender.is_none() {
                                sender = Some(osc_sender().await?);
                            }
                            refresh_avatar_context(&mut avatar_context).await;
                            report_runtime_if_changed(
                                &mut write_half,
                                &mut last_report,
                                avatar_context.as_ref(),
                            )
                            .await?;
                            start_transition_automation(
                                &mut scripts,
                                sender.as_ref().expect("sender created").clone(),
                                &config,
                                new_sleeping,
                                gogo_enabled,
                                avatar_context.clone(),
                            );
                        }

                        if pose_changed
                            && sleeping
                            && gogo_enabled
                            && pose != SleepingPose::Unknown
                        {
                            if sender.is_none() {
                                sender = Some(osc_sender().await?);
                            }
                            refresh_avatar_context(&mut avatar_context).await;
                            report_runtime_if_changed(
                                &mut write_half,
                                &mut last_report,
                                avatar_context.as_ref(),
                            )
                            .await?;
                            start_pose_automation(
                                &mut scripts,
                                sender.as_ref().expect("sender created").clone(),
                                pose,
                                avatar_context.clone(),
                            );
                        }
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

    log_example_semantic_resolution();
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
    fn parses_legacy_sleep_sequence() {
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
        assert_eq!(config.on_sleep.to_script().actions.len(), 3);
        assert!(config.on_wake.to_script().actions.is_empty());
    }

    #[test]
    fn parses_versioned_sleep_script() {
        let json = r#"
        {
          "publish_sleep_parameter": true,
          "on_sleep": {
            "version": 1,
            "actions": [
              {
                "type": "send",
                "address": "/avatar/parameters/Go/Stationary",
                "value": { "type": "bool", "value": true }
              }
            ]
          },
          "on_wake": { "version": 1, "actions": [] }
        }
        "#;

        let config: OscAutomationConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.on_sleep.to_script().version, CURRENT_SCRIPT_VERSION);
        assert_eq!(config.on_sleep.to_script().actions.len(), 1);
    }

    #[test]
    fn custom_transition_scripts_are_independent_of_gogo() {
        let config: OscAutomationConfig = serde_json::from_str(
            r#"{
                "on_sleep": [
                    {
                        "type": "send",
                        "address": "/avatar/parameters/UserSleep",
                        "value": { "type": "bool", "value": true }
                    }
                ],
                "on_wake": [
                    {
                        "type": "send",
                        "address": "/avatar/parameters/UserSleep",
                        "value": { "type": "bool", "value": false }
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(custom_transition_script(&config, true).actions.len(), 1);
        assert_eq!(custom_transition_script(&config, false).actions.len(), 1);
    }

    #[test]
    fn only_logical_sleep_edges_trigger_sequences() {
        assert_eq!(transition_edge(false, SleepPhase::EnteringSleep), Some(true));
        assert_eq!(transition_edge(true, SleepPhase::Sleeping), None);
        assert_eq!(transition_edge(true, SleepPhase::Waking), Some(false));
        assert_eq!(transition_edge(false, SleepPhase::Awake), None);
    }

    #[test]
    fn runtime_report_without_avatar_is_connected_but_empty() {
        assert_eq!(
            runtime_report(None),
            WorkerReport::OscRuntime {
                connected: true,
                avatar_id: None,
                avatar_parameter_count: 0,
                gogo_detected: false,
            }
        );
    }
}
