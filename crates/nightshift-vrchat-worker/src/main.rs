use std::{
    env,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use nightshift_protocol::{
    ClientMessage, InviteState, PROTOCOL_VERSION, ServerMessage, StateEvent, VrchatStatus,
    WorkerReport,
};
use nightshift_vrchat_api::{
    SessionCookies, VrchatApiClient, VrchatNotification, parse_pipeline_notification, pipeline_url,
};
use nightshift_vrchat_session::{VrchatSession, VrchatSessionStore};
use nightshift_vrchat_status::{
    StatusAutomationConfig, sleeping_phase, target_for_transition,
};
use nightshift_vrchat_worker::{
    invite_bridge::{
        InviteHandlingOutcome, InviteHandlingResult, handle_invite_notification,
    },
    invite_execute::InviteExecutionOutcome,
};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const STATUS_DEBOUNCE: Duration = Duration::from_secs(3);
const STATUS_MIN_INTERVAL: Duration = Duration::from_secs(10);
const PIPELINE_RECONNECT_DELAY: Duration = Duration::from_secs(10);
const PIPELINE_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

struct AbortTask(Option<JoinHandle<()>>);

impl AbortTask {
    fn replace(&mut self, task: JoinHandle<()>) {
        self.abort();
        self.0 = Some(task);
    }

    fn abort(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }

    fn is_pending(&self) -> bool {
        self.0.is_some()
    }
}

impl Drop for AbortTask {
    fn drop(&mut self) {
        self.abort();
    }
}

struct RuntimeRefresh {
    report: WorkerReport,
    pipeline_auth_token: Option<String>,
    session_cookies: Option<SessionCookies>,
}

struct StatusTaskResult {
    generation: u64,
    result: Result<RuntimeRefresh>,
}

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

fn disconnected_report() -> WorkerReport {
    WorkerReport::VrchatRuntime {
        connected: false,
        display_name: None,
        status: VrchatStatus::Offline,
    }
}

fn disconnected_refresh() -> RuntimeRefresh {
    RuntimeRefresh {
        report: disconnected_report(),
        pipeline_auth_token: None,
        session_cookies: None,
    }
}

fn session_cookies(session: &VrchatSession) -> SessionCookies {
    SessionCookies {
        auth: session.auth_cookie.clone(),
        two_factor_auth: session.two_factor_cookie.clone(),
    }
}

fn status_config(settings: &nightshift_protocol::SettingsState) -> StatusAutomationConfig {
    StatusAutomationConfig {
        enabled: settings.vrchat_status_automation_enabled,
        sleep_status: settings.sleep_vrchat_status,
        wake_status: settings.wake_vrchat_status,
    }
}

fn report_runtime(report: &WorkerReport) -> (bool, VrchatStatus) {
    match report {
        WorkerReport::VrchatRuntime {
            connected, status, ..
        } => (*connected, *status),
        _ => (false, VrchatStatus::Offline),
    }
}

fn status_update_delay(last_write: Option<Instant>) -> Duration {
    let rate_limit_wait = last_write
        .map(|last| STATUS_MIN_INTERVAL.saturating_sub(last.elapsed()))
        .unwrap_or_default();
    STATUS_DEBOUNCE.max(rate_limit_wait)
}

fn pending_retarget(
    pending: bool,
    sleeping: bool,
    connected: bool,
    current_status: VrchatStatus,
    automation: StatusAutomationConfig,
) -> Option<VrchatStatus> {
    if !pending || !automation.enabled || !connected {
        return None;
    }

    let target = if sleeping {
        automation.sleep_status
    } else {
        automation.wake_status
    };
    (target != current_status).then_some(target)
}

fn cancel_pending_status_on_disconnect(
    connected: bool,
    status_generation: &mut u64,
    status_task: &mut AbortTask,
) {
    if !connected && status_task.is_pending() {
        *status_generation = status_generation.wrapping_add(1);
        status_task.abort();
    }
}

fn sync_invite_session(
    current: &mut Option<SessionCookies>,
    next: Option<SessionCookies>,
    tasks: &mut JoinSet<Result<InviteHandlingResult>>,
) {
    if current.as_ref() == next.as_ref() {
        return;
    }
    tasks.abort_all();
    *current = next;
}

fn sync_invite_state(
    current: &mut InviteState,
    next: InviteState,
    tasks: &mut JoinSet<Result<InviteHandlingResult>>,
) {
    if current.enabled != next.enabled || current.profile != next.profile {
        tasks.abort_all();
    }
    *current = next;
}

fn log_invite_result(result: InviteHandlingResult) {
    let InviteHandlingResult {
        notification_id,
        sender_user_id,
        sender_username,
        outcome,
    } = result;
    let sender_username = sender_username.as_deref().unwrap_or("<unknown>");

    match outcome {
        InviteHandlingOutcome::ConfigurationUnavailable(profile) => {
            warn!(
                ?profile,
                notification_id = %notification_id,
                sender_user_id = %sender_user_id,
                sender_username = %sender_username,
                "invite automation profile requires configuration that is not available yet"
            );
        }
        InviteHandlingOutcome::Execution(outcome) => match outcome {
            InviteExecutionOutcome::None => {}
            InviteExecutionOutcome::AcceptedRequest => {
                info!(
                    notification_id = %notification_id,
                    sender_user_id = %sender_user_id,
                    sender_username = %sender_username,
                    "accepted VRChat invite request"
                );
            }
            InviteExecutionOutcome::Declined => {
                info!(
                    notification_id = %notification_id,
                    sender_user_id = %sender_user_id,
                    sender_username = %sender_username,
                    "declined VRChat invite notification"
                );
            }
            InviteExecutionOutcome::InviteRefused { message } => {
                warn!(
                    notification_id = %notification_id,
                    sender_user_id = %sender_user_id,
                    sender_username = %sender_username,
                    vrchat_message = message.as_deref().unwrap_or("<none>"),
                    "VRChat refused invite-request acceptance; original request was left visible"
                );
            }
            InviteExecutionOutcome::MissingCurrentInstance => {
                warn!(
                    notification_id = %notification_id,
                    sender_user_id = %sender_user_id,
                    sender_username = %sender_username,
                    "invite request matched policy but current VRChat instance is unavailable"
                );
            }
            InviteExecutionOutcome::UnsupportedIncomingInviteAcceptance => {
                info!(
                    notification_id = %notification_id,
                    sender_user_id = %sender_user_id,
                    sender_username = %sender_username,
                    "incoming VRChat invite matched accept policy, but automatic joining is not implemented"
                );
            }
        },
    }
}

fn pipeline_error_message(frame: &str) -> Option<String> {
    let value: Value = serde_json::from_str(frame).ok()?;
    let error = value.get("err")?;
    match error {
        Value::String(message) => Some(message.clone()),
        Value::Object(fields) => fields
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| Some("VRChat pipeline returned an error".into())),
        _ => Some("VRChat pipeline returned an error".into()),
    }
}

async fn pipeline_loop(
    auth_token: String,
    events: mpsc::UnboundedSender<VrchatNotification>,
) {
    loop {
        let url = pipeline_url(&auth_token);
        let connection = tokio::time::timeout(
            PIPELINE_CONNECT_TIMEOUT,
            connect_async(url.as_str()),
        )
        .await;

        let mut socket = match connection {
            Ok(Ok((socket, _response))) => {
                info!("VRChat pipeline connected");
                socket
            }
            Ok(Err(error)) => {
                warn!(%error, "VRChat pipeline connection failed");
                tokio::time::sleep(PIPELINE_RECONNECT_DELAY).await;
                continue;
            }
            Err(_) => {
                warn!("VRChat pipeline connection timed out");
                tokio::time::sleep(PIPELINE_RECONNECT_DELAY).await;
                continue;
            }
        };

        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let frame = text.as_str();
                    if let Some(message) = pipeline_error_message(frame) {
                        warn!(%message, "VRChat pipeline rejected the connection");
                        break;
                    }

                    match parse_pipeline_notification(frame) {
                        Ok(Some(notification)) => {
                            if events.send(notification).is_err() {
                                return;
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            warn!(%error, "ignored malformed VRChat pipeline frame");
                        }
                    }
                }
                Some(Ok(Message::Close(_))) => {
                    info!("VRChat pipeline closed");
                    break;
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    warn!(%error, "VRChat pipeline read failed");
                    break;
                }
                None => {
                    info!("VRChat pipeline ended");
                    break;
                }
            }
        }

        tokio::time::sleep(PIPELINE_RECONNECT_DELAY).await;
    }
}

fn sync_pipeline_task(
    task: &mut AbortTask,
    current_auth_token: &mut Option<String>,
    next_auth_token: Option<String>,
    events: &mpsc::UnboundedSender<VrchatNotification>,
) {
    if current_auth_token.as_deref() == next_auth_token.as_deref() {
        return;
    }

    task.abort();
    *current_auth_token = next_auth_token.clone();

    if let Some(auth_token) = next_auth_token {
        let events = events.clone();
        task.replace(tokio::spawn(async move {
            pipeline_loop(auth_token, events).await;
        }));
    }
}

async fn load_session(store: VrchatSessionStore) -> Result<Option<VrchatSession>> {
    tokio::task::spawn_blocking(move || store.load())
        .await
        .context("VRChat session keyring task stopped")?
}

async fn save_session(store: VrchatSessionStore, session: VrchatSession) -> Result<()> {
    tokio::task::spawn_blocking(move || store.save(&session))
        .await
        .context("VRChat session keyring task stopped")?
}

async fn validated_user(
    api: &VrchatApiClient,
    store: &VrchatSessionStore,
) -> Result<Option<(VrchatSession, nightshift_vrchat_api::CurrentUserSummary)>> {
    let mut session = match load_session(store.clone()).await? {
        Some(session) => session,
        None => return Ok(None),
    };
    let user = api.current_user(&session_cookies(&session)).await?;

    if let Some(expected_user_id) = session.user_id.as_deref() {
        if expected_user_id != user.id {
            bail!(
                "VRChat keyring session account mismatch: expected {expected_user_id}, got {}",
                user.id
            );
        }
    } else {
        session.user_id = Some(user.id.clone());
        if let Err(error) = save_session(store.clone(), session.clone()).await {
            warn!(%error, "failed to attach VRChat user id to stored session");
        }
    }

    Ok(Some((session, user)))
}

async fn refresh_runtime(
    api: &VrchatApiClient,
    store: &VrchatSessionStore,
) -> RuntimeRefresh {
    let (session, user) = match validated_user(api, store).await {
        Ok(Some(value)) => value,
        Ok(None) => return disconnected_refresh(),
        Err(error) => {
            warn!(%error, "VRChat session validation failed");
            return disconnected_refresh();
        }
    };
    let cookies = session_cookies(&session);

    RuntimeRefresh {
        report: WorkerReport::VrchatRuntime {
            connected: true,
            display_name: Some(user.display_name),
            status: user.status,
        },
        pipeline_auth_token: Some(cookies.auth.clone()),
        session_cookies: Some(cookies),
    }
}

async fn set_runtime_status(
    api: VrchatApiClient,
    store: VrchatSessionStore,
    target: VrchatStatus,
) -> Result<RuntimeRefresh> {
    let Some((session, user)) = validated_user(&api, &store).await? else {
        bail!("no VRChat session is stored");
    };
    let cookies = session_cookies(&session);

    if user.status != target {
        api.set_status(&cookies, &user.id, target).await?;
    }

    Ok(RuntimeRefresh {
        report: WorkerReport::VrchatRuntime {
            connected: true,
            display_name: Some(user.display_name),
            status: target,
        },
        pipeline_auth_token: Some(cookies.auth.clone()),
        session_cookies: Some(cookies),
    })
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

async fn report_if_changed(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    previous: &mut Option<WorkerReport>,
    report: WorkerReport,
) -> Result<()> {
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

fn schedule_status_update(
    task: &mut AbortTask,
    result_tx: mpsc::UnboundedSender<StatusTaskResult>,
    api: VrchatApiClient,
    store: VrchatSessionStore,
    target: VrchatStatus,
    last_write: Option<Instant>,
    generation: u64,
) {
    let delay = status_update_delay(last_write);
    task.replace(tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let result = set_runtime_status(api, store, target).await;
        let _ = result_tx.send(StatusTaskResult { generation, result });
    }));
}

async fn run_connection(stream: UnixStream) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    write_client_message(
        &mut write_half,
        &ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            client: "nightshift-vrchat".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        },
    )
    .await?;

    let api = VrchatApiClient::new()?;
    let store = VrchatSessionStore::default();
    let mut last_report: Option<WorkerReport> = None;
    let mut interval = tokio::time::interval(REFRESH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut line = String::new();
    let mut handshake_complete = false;

    let mut sleeping = false;
    let mut vrchat_connected = false;
    let mut current_status = VrchatStatus::Offline;
    let mut automation = StatusAutomationConfig::default();
    let mut last_status_write: Option<Instant> = None;
    let mut status_generation = 0_u64;
    let mut status_task = AbortTask(None);
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<StatusTaskResult>();

    let mut pipeline_task = AbortTask(None);
    let mut pipeline_auth_token: Option<String> = None;
    let (pipeline_tx, mut pipeline_rx) = mpsc::unbounded_channel::<VrchatNotification>();

    let mut invite_state = InviteState::default();
    let mut active_session: Option<SessionCookies> = None;
    let mut invite_tasks = JoinSet::<Result<InviteHandlingResult>>::new();

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
                                "daemon protocol mismatch: vrchat={}, daemon={protocol}",
                                PROTOCOL_VERSION
                            );
                        }
                        handshake_complete = true;
                        info!(protocol, "Nightshift VRChat worker connected to daemon");
                        let RuntimeRefresh {
                            report,
                            pipeline_auth_token: next_pipeline_auth_token,
                            session_cookies: next_session,
                        } = refresh_runtime(&api, &store).await;
                        (vrchat_connected, current_status) = report_runtime(&report);
                        sync_invite_session(
                            &mut active_session,
                            next_session,
                            &mut invite_tasks,
                        );
                        sync_pipeline_task(
                            &mut pipeline_task,
                            &mut pipeline_auth_token,
                            next_pipeline_auth_token,
                            &pipeline_tx,
                        );
                        report_if_changed(&mut write_half, &mut last_report, report).await?;
                    }
                    ServerMessage::FullState { state } => {
                        // Snapshot is synchronization, not a fresh Sleep/Wake edge.
                        sleeping = sleeping_phase(state.sleep.phase);
                        automation = status_config(&state.settings);
                        sync_invite_state(&mut invite_state, state.invites, &mut invite_tasks);
                    }
                    ServerMessage::Event { event } => match event {
                        StateEvent::SettingsChanged(settings) => {
                            let previous = automation;
                            let pending = status_task.is_pending();
                            automation = status_config(&settings);

                            if pending && automation != previous {
                                status_generation = status_generation.wrapping_add(1);
                                status_task.abort();
                                if let Some(target) = pending_retarget(
                                    true,
                                    sleeping,
                                    vrchat_connected,
                                    current_status,
                                    automation,
                                ) {
                                    schedule_status_update(
                                        &mut status_task,
                                        status_tx.clone(),
                                        api.clone(),
                                        store.clone(),
                                        target,
                                        last_status_write,
                                        status_generation,
                                    );
                                }
                            }
                        }
                        StateEvent::VrchatChanged(state) => {
                            vrchat_connected = state.connected;
                            current_status = state.status;
                            cancel_pending_status_on_disconnect(
                                vrchat_connected,
                                &mut status_generation,
                                &mut status_task,
                            );
                            if !vrchat_connected {
                                sync_invite_session(
                                    &mut active_session,
                                    None,
                                    &mut invite_tasks,
                                );
                                sync_pipeline_task(
                                    &mut pipeline_task,
                                    &mut pipeline_auth_token,
                                    None,
                                    &pipeline_tx,
                                );
                            }
                        }
                        StateEvent::SleepChanged(state) => {
                            let target = target_for_transition(
                                sleeping,
                                state.phase,
                                vrchat_connected,
                                current_status,
                                automation,
                            );
                            sleeping = sleeping_phase(state.phase);
                            if let Some(target) = target {
                                status_generation = status_generation.wrapping_add(1);
                                schedule_status_update(
                                    &mut status_task,
                                    status_tx.clone(),
                                    api.clone(),
                                    store.clone(),
                                    target,
                                    last_status_write,
                                    status_generation,
                                );
                            }
                        }
                        StateEvent::InvitesChanged(state) => {
                            sync_invite_state(&mut invite_state, state, &mut invite_tasks);
                        }
                        _ => {}
                    },
                    ServerMessage::Error { message } => {
                        warn!(%message, "daemon rejected VRChat worker message");
                    }
                }
            }
            _ = interval.tick(), if handshake_complete => {
                let RuntimeRefresh {
                    report,
                    pipeline_auth_token: next_pipeline_auth_token,
                    session_cookies: next_session,
                } = refresh_runtime(&api, &store).await;
                (vrchat_connected, current_status) = report_runtime(&report);
                cancel_pending_status_on_disconnect(
                    vrchat_connected,
                    &mut status_generation,
                    &mut status_task,
                );
                sync_invite_session(
                    &mut active_session,
                    next_session,
                    &mut invite_tasks,
                );
                sync_pipeline_task(
                    &mut pipeline_task,
                    &mut pipeline_auth_token,
                    next_pipeline_auth_token,
                    &pipeline_tx,
                );
                report_if_changed(&mut write_half, &mut last_report, report).await?;
            }
            Some(task_result) = status_rx.recv() => {
                if task_result.generation != status_generation {
                    continue;
                }
                status_task.0 = None;
                match task_result.result {
                    Ok(refresh) => {
                        last_status_write = Some(Instant::now());
                        let RuntimeRefresh {
                            report,
                            pipeline_auth_token: next_pipeline_auth_token,
                            session_cookies: next_session,
                        } = refresh;
                        (vrchat_connected, current_status) = report_runtime(&report);
                        sync_invite_session(
                            &mut active_session,
                            next_session,
                            &mut invite_tasks,
                        );
                        sync_pipeline_task(
                            &mut pipeline_task,
                            &mut pipeline_auth_token,
                            next_pipeline_auth_token,
                            &pipeline_tx,
                        );
                        report_if_changed(&mut write_half, &mut last_report, report).await?;
                    }
                    Err(error) => {
                        warn!(%error, "VRChat sleep/wake status update failed");
                    }
                }
            }
            Some(notification) = pipeline_rx.recv() => {
                info!(
                    notification_id = %notification.id,
                    sender_user_id = %notification.sender_user_id,
                    sender_username = notification.sender_username.as_deref().unwrap_or("<unknown>"),
                    kind = ?notification.kind,
                    "received VRChat invite notification"
                );

                if let Some(cookies) = active_session.clone() {
                    let api = api.clone();
                    let state = invite_state.clone();
                    invite_tasks.spawn(async move {
                        handle_invite_notification(api, cookies, state, notification).await
                    });
                } else {
                    warn!(
                        notification_id = %notification.id,
                        "ignored VRChat invite notification because no validated session is active"
                    );
                }
            }
            Some(join_result) = invite_tasks.join_next(), if !invite_tasks.is_empty() => {
                match join_result {
                    Ok(Ok(result)) => log_invite_result(result),
                    Ok(Err(error)) => {
                        warn!(%error, "VRChat invite automation task failed without taking a fallback action");
                    }
                    Err(error) if error.is_cancelled() => {}
                    Err(error) => {
                        warn!(%error, "VRChat invite automation task stopped unexpectedly");
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
                .unwrap_or_else(|_| "nightshift_vrchat=info".into()),
        )
        .init();

    let path = socket_path()?;
    loop {
        match UnixStream::connect(&path).await {
            Ok(stream) => {
                if let Err(error) = run_connection(stream).await {
                    warn!(%error, "VRChat worker disconnected from daemon");
                }
            }
            Err(error) => {
                warn!(%error, path = %path.display(), "Nightshift daemon unavailable");
            }
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nightshift_protocol::SettingsState;

    #[test]
    fn converts_stored_session_to_vrchat_cookie_names() {
        let session = VrchatSession {
            auth_cookie: "auth-cookie".into(),
            two_factor_cookie: Some("2fa-cookie".into()),
            user_id: None,
        };
        assert_eq!(
            session_cookies(&session).cookie_header(),
            "auth=auth-cookie; twoFactorAuth=2fa-cookie"
        );
    }

    #[test]
    fn disconnected_report_is_explicitly_offline() {
        assert_eq!(
            disconnected_report(),
            WorkerReport::VrchatRuntime {
                connected: false,
                display_name: None,
                status: VrchatStatus::Offline,
            }
        );
    }

    #[test]
    fn pipeline_errors_extract_only_safe_message() {
        assert_eq!(
            pipeline_error_message(r#"{"err":"bad auth"}"#).as_deref(),
            Some("bad auth")
        );
        assert_eq!(
            pipeline_error_message(r#"{"err":{"message":"expired"}}"#).as_deref(),
            Some("expired")
        );
        assert_eq!(pipeline_error_message(r#"{"type":"notification"}"#), None);
    }

    #[test]
    fn settings_map_to_status_policy() {
        let mut settings = SettingsState::default();
        settings.vrchat_status_automation_enabled = true;
        settings.sleep_vrchat_status = VrchatStatus::Busy;
        settings.wake_vrchat_status = VrchatStatus::JoinMe;
        assert_eq!(
            status_config(&settings),
            StatusAutomationConfig {
                enabled: true,
                sleep_status: VrchatStatus::Busy,
                wake_status: VrchatStatus::JoinMe,
            }
        );
    }

    #[test]
    fn pending_sleep_write_retargets_to_new_sleep_status() {
        assert_eq!(
            pending_retarget(
                true,
                true,
                true,
                VrchatStatus::Active,
                StatusAutomationConfig {
                    enabled: true,
                    sleep_status: VrchatStatus::Busy,
                    wake_status: VrchatStatus::Active,
                },
            ),
            Some(VrchatStatus::Busy)
        );
    }

    #[test]
    fn settings_change_without_pending_edge_does_not_create_write() {
        assert_eq!(
            pending_retarget(
                false,
                true,
                true,
                VrchatStatus::Active,
                StatusAutomationConfig {
                    enabled: true,
                    sleep_status: VrchatStatus::Busy,
                    wake_status: VrchatStatus::Active,
                },
            ),
            None
        );
    }

    #[test]
    fn disabling_automation_cancels_pending_retarget() {
        assert_eq!(
            pending_retarget(
                true,
                true,
                true,
                VrchatStatus::Active,
                StatusAutomationConfig {
                    enabled: false,
                    sleep_status: VrchatStatus::Busy,
                    wake_status: VrchatStatus::Active,
                },
            ),
            None
        );
    }

    #[test]
    fn pending_retarget_skips_already_matching_status() {
        assert_eq!(
            pending_retarget(
                true,
                true,
                true,
                VrchatStatus::Busy,
                StatusAutomationConfig {
                    enabled: true,
                    sleep_status: VrchatStatus::Busy,
                    wake_status: VrchatStatus::Active,
                },
            ),
            None
        );
    }

    #[tokio::test]
    async fn disconnect_aborts_pending_status_task() {
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = ran.clone();
        let mut task = AbortTask(Some(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, std::sync::atomic::Ordering::Release);
        })));
        let mut generation = 7;

        cancel_pending_status_on_disconnect(false, &mut generation, &mut task);
        tokio::time::sleep(Duration::from_millis(80)).await;

        assert_eq!(generation, 8);
        assert!(!task.is_pending());
        assert!(!ran.load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    fn status_writes_wait_at_least_for_debounce() {
        assert!(status_update_delay(None) >= STATUS_DEBOUNCE);
    }

    #[test]
    fn recent_status_write_extends_delay_for_rate_limit() {
        let delay = status_update_delay(Some(Instant::now() - Duration::from_secs(5)));
        assert!(delay >= Duration::from_secs(4));
        assert!(delay <= Duration::from_secs(6));
    }
}
