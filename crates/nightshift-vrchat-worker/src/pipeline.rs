use super::*;

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

pub(crate) async fn pipeline_loop(
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
            Ok(Err(_)) => {
                // Do not log the websocket error object: depending on the implementation/error
                // variant it may retain request details containing the authToken query parameter.
                warn!("VRChat pipeline connection failed; retrying");
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
                        let message = redact_pipeline_secret(&message, &auth_token);
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
                Some(Err(_)) => {
                    // Keep websocket internals out of logs for the same reason as connect errors.
                    warn!("VRChat pipeline read failed; reconnecting");
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

pub(crate) fn sync_pipeline_task(
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
