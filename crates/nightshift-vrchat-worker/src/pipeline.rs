use super::*;
use nightshift_vrchat_status::VrchatPresence;
use std::sync::OnceLock;
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::{HeaderValue, Request, header::USER_AGENT},
};

const HEALTHY_CONNECTION_DURATION: Duration = Duration::from_secs(30);
const FAILURE_REVALIDATE_THRESHOLD: u32 = 3;

#[derive(Clone, Debug)]
pub(crate) enum PipelineEvent {
    Notification(VrchatNotification),
    RevalidateSession,
}

#[derive(Clone)]
struct LiveUserPresence {
    user_id: String,
    presence: VrchatPresence,
}

static LIVE_USER_PRESENCE: OnceLock<Mutex<Option<LiveUserPresence>>> = OnceLock::new();

fn live_presence_slot() -> &'static Mutex<Option<LiveUserPresence>> {
    LIVE_USER_PRESENCE.get_or_init(|| Mutex::new(None))
}

pub(crate) fn latest_live_user_presence(user_id: &str) -> Option<VrchatPresence> {
    let slot = live_presence_slot().lock().ok()?;
    let live = slot.as_ref()?;
    (live.user_id == user_id).then(|| live.presence.clone())
}

fn clear_live_user_presence() {
    if let Ok(mut slot) = live_presence_slot().lock() {
        *slot = None;
    }
}

fn parse_pipeline_status(value: &str) -> Option<VrchatStatus> {
    match value {
        "active" => Some(VrchatStatus::Active),
        "join me" => Some(VrchatStatus::JoinMe),
        "ask me" => Some(VrchatStatus::AskMe),
        "busy" => Some(VrchatStatus::Busy),
        "offline" => Some(VrchatStatus::Offline),
        _ => None,
    }
}

fn capture_user_update(frame: &str) -> Result<bool> {
    let envelope: Value = serde_json::from_str(frame).context("decoding VRChat pipeline frame")?;
    if envelope.get("type").and_then(Value::as_str) != Some("user-update") {
        return Ok(false);
    }

    let content = envelope
        .get("content")
        .context("VRChat user-update frame is missing content")?;
    let content: Value = match content {
        Value::String(content) => serde_json::from_str(content)
            .context("decoding stringified VRChat user-update")?,
        other => other.clone(),
    };
    let user = content
        .get("user")
        .context("VRChat user-update frame is missing user")?;
    let user_id = user
        .get("id")
        .and_then(Value::as_str)
        .context("VRChat user-update frame is missing user id")?;
    let status_wire = user
        .get("status")
        .and_then(Value::as_str)
        .context("VRChat user-update frame is missing status")?;
    let status = parse_pipeline_status(status_wire)
        .with_context(|| format!("unknown VRChat pipeline status {status_wire:?}"))?;
    let status_description = user
        .get("statusDescription")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let presence = VrchatPresence::new(status, status_description);

    if let Ok(mut slot) = live_presence_slot().lock() {
        *slot = Some(LiveUserPresence {
            user_id: user_id.to_owned(),
            presence: presence.clone(),
        });
    }
    info!(
        user_id,
        status = ?presence.status,
        "VRChat pipeline updated current-user presence"
    );
    Ok(true)
}

fn pipeline_request(auth_token: &str) -> Result<Request<()>> {
    let mut request = pipeline_url(auth_token)
        .into_client_request()
        .context("building VRChat pipeline request")?;
    let user_agent = HeaderValue::from_str(&format!(
        "Nightshift/{} (https://github.com/Chineko/Nightshift)",
        env!("CARGO_PKG_VERSION")
    ))
    .context("building VRChat pipeline user agent")?;
    request.headers_mut().insert(USER_AGENT, user_agent);
    Ok(request)
}

pub(crate) fn pipeline_error_message(frame: &str) -> Option<String> {
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

pub(crate) fn redact_pipeline_secret(message: &str, auth_token: &str) -> String {
    if auth_token.is_empty() {
        return message.to_owned();
    }

    let mut redacted = message.replace(auth_token, "<redacted>");
    if let Some((_, encoded)) = pipeline_url(auth_token).split_once("authToken=") {
        redacted = redacted.replace(encoded, "<redacted>");
    }
    redacted
}

fn note_connection_end(
    consecutive_failures: &mut u32,
    opened_at: Option<Instant>,
) -> bool {
    if opened_at.is_some_and(|opened| opened.elapsed() >= HEALTHY_CONNECTION_DURATION) {
        *consecutive_failures = 0;
        return false;
    }

    *consecutive_failures = consecutive_failures.saturating_add(1);
    if *consecutive_failures >= FAILURE_REVALIDATE_THRESHOLD {
        *consecutive_failures = 0;
        true
    } else {
        false
    }
}

fn request_session_revalidation(events: &mpsc::UnboundedSender<PipelineEvent>) -> bool {
    warn!(
        threshold = FAILURE_REVALIDATE_THRESHOLD,
        "VRChat pipeline repeatedly failed before becoming healthy; revalidating session"
    );
    events.send(PipelineEvent::RevalidateSession).is_ok()
}

pub(crate) async fn pipeline_loop(
    auth_token: String,
    events: mpsc::UnboundedSender<PipelineEvent>,
) {
    let mut consecutive_failures = 0_u32;

    loop {
        let request = match pipeline_request(&auth_token) {
            Ok(request) => request,
            Err(error) => {
                warn!(%error, "failed to build VRChat pipeline request; retrying");
                if note_connection_end(&mut consecutive_failures, None)
                    && !request_session_revalidation(&events)
                {
                    return;
                }
                tokio::time::sleep(PIPELINE_RECONNECT_DELAY).await;
                continue;
            }
        };
        let connection = tokio::time::timeout(
            PIPELINE_CONNECT_TIMEOUT,
            connect_async(request),
        )
        .await;

        let (mut socket, opened_at) = match connection {
            Ok(Ok((socket, _response))) => {
                info!("VRChat pipeline connected");
                (socket, Some(Instant::now()))
            }
            Ok(Err(error)) => {
                let message = redact_pipeline_secret(&error.to_string(), &auth_token);
                warn!(%message, "VRChat pipeline connection failed; retrying");
                if note_connection_end(&mut consecutive_failures, None)
                    && !request_session_revalidation(&events)
                {
                    return;
                }
                tokio::time::sleep(PIPELINE_RECONNECT_DELAY).await;
                continue;
            }
            Err(_) => {
                warn!("VRChat pipeline connection timed out");
                if note_connection_end(&mut consecutive_failures, None)
                    && !request_session_revalidation(&events)
                {
                    return;
                }
                tokio::time::sleep(PIPELINE_RECONNECT_DELAY).await;
                continue;
            }
        };

        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let frame = text.as_str();
                    if let Some(message) = pipeline_error_message(frame) {
                        let message = redact_pipeline_secret(&message, &auth_token);
                        warn!(%message, "VRChat pipeline rejected the connection");
                        break;
                    }

                    match capture_user_update(frame) {
                        Ok(true) => continue,
                        Ok(false) => {}
                        Err(error) => {
                            warn!(%error, "ignored malformed VRChat current-user update");
                            continue;
                        }
                    }

                    match parse_pipeline_notification(frame) {
                        Ok(Some(notification)) => {
                            if events
                                .send(PipelineEvent::Notification(notification))
                                .is_err()
                            {
                                return;
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            warn!(%error, "ignored malformed VRChat pipeline frame");
                        }
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    if let Some(frame) = frame {
                        info!(
                            close_code = ?frame.code,
                            reason = %frame.reason,
                            "VRChat pipeline closed"
                        );
                    } else {
                        info!("VRChat pipeline closed");
                    }
                    break;
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    let message = redact_pipeline_secret(&error.to_string(), &auth_token);
                    warn!(%message, "VRChat pipeline read failed; reconnecting");
                    break;
                }
                None => {
                    info!("VRChat pipeline ended");
                    break;
                }
            }
        }

        if note_connection_end(&mut consecutive_failures, opened_at)
            && !request_session_revalidation(&events)
        {
            return;
        }
        tokio::time::sleep(PIPELINE_RECONNECT_DELAY).await;
    }
}

pub(crate) fn sync_pipeline_task(
    task: &mut AbortTask,
    current_auth_token: &mut Option<String>,
    next_auth_token: Option<String>,
    events: &mpsc::UnboundedSender<PipelineEvent>,
) {
    if current_auth_token.as_deref() == next_auth_token.as_deref() {
        return;
    }

    task.abort();
    clear_live_user_presence();
    *current_auth_token = next_auth_token.clone();

    if let Some(auth_token) = next_auth_token {
        let events = events.clone();
        task.replace(tokio::spawn(async move {
            pipeline_loop(auth_token, events).await;
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unhealthy_pipeline_streak_requests_revalidation_on_third_failure() {
        let mut failures = 0;
        assert!(!note_connection_end(&mut failures, None));
        assert!(!note_connection_end(&mut failures, None));
        assert!(note_connection_end(&mut failures, None));
        assert_eq!(failures, 0);
    }

    #[tokio::test]
    async fn healthy_pipeline_resets_failure_streak() {
        let mut failures = 2;
        let opened_at = Instant::now() - HEALTHY_CONNECTION_DURATION - Duration::from_secs(1);
        assert!(!note_connection_end(&mut failures, Some(opened_at)));
        assert_eq!(failures, 0);
    }
}
