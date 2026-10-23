use std::{error::Error, fmt};

use anyhow::{Context, Result, bail};
use reqwest::{StatusCode, header::COOKIE};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{BASE_URL, SessionCookies, VrchatApiClient};

const PIPELINE_URL: &str = "wss://pipeline.vrchat.cloud/";
const PAGE_SIZE: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationKind {
    Invite,
    RequestInvite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VrchatNotification {
    pub id: String,
    pub kind: NotificationKind,
    pub sender_user_id: String,
    pub sender_username: Option<String>,
}

#[derive(Debug)]
pub struct InviteRefused {
    pub message: Option<String>,
    pub body: String,
}

impl fmt::Display for InviteRefused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(message) => write!(formatter, "VRChat refused the invite: {message}"),
            None => formatter.write_str("VRChat refused the invite"),
        }
    }
}

impl Error for InviteRefused {}

#[derive(Debug, Deserialize)]
struct NotificationHeader {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct WireNotification {
    id: String,
    #[serde(rename = "senderUserId")]
    sender_user_id: String,
    #[serde(rename = "senderUsername", default)]
    sender_username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FriendRecord {
    id: String,
}

#[derive(Debug, Deserialize)]
struct FavoriteRecord {
    #[serde(rename = "favoriteId")]
    favorite_id: String,
}

#[derive(Debug, Serialize)]
struct FriendPageQuery {
    offline: bool,
    n: usize,
    offset: usize,
}

#[derive(Debug, Serialize)]
struct FavoritePageQuery<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    n: usize,
    offset: usize,
}

#[derive(Debug, Serialize)]
struct InviteBody<'a> {
    #[serde(rename = "instanceId")]
    instance_id: &'a str,
    #[serde(rename = "messageSlot", skip_serializing_if = "Option::is_none")]
    message_slot: Option<u8>,
}

#[derive(Debug, Serialize)]
struct DeclineBody {
    #[serde(rename = "responseSlot", skip_serializing_if = "Option::is_none")]
    response_slot: Option<u8>,
}

pub fn pipeline_url(auth_token: &str) -> String {
    format!("{PIPELINE_URL}?authToken={}", urlencoding::encode(auth_token))
}

/// Parse one VRChat pipeline frame and return only invite/invite-request notifications.
/// Other pipeline event types and other notification kinds are intentionally ignored.
pub fn parse_pipeline_notification(frame: &str) -> Result<Option<VrchatNotification>> {
    let envelope: Value = serde_json::from_str(frame).context("decoding VRChat pipeline frame")?;
    let Some(event_type) = envelope.get("type").and_then(Value::as_str) else {
        if envelope.get("err").is_some() {
            return Ok(None);
        }
        bail!("VRChat pipeline frame is missing an event type");
    };
    if event_type != "notification" {
        return Ok(None);
    }

    let content = envelope
        .get("content")
        .context("VRChat notification frame is missing content")?;
    let notification: Value = match content {
        Value::String(content) => serde_json::from_str(content)
            .context("decoding stringified VRChat notification")?,
        other => other.clone(),
    };

    // Filter the notification kind before requiring invite-specific fields. VRChat sends many
    // notification shapes through this same event type and unrelated notifications may not have
    // senderUserId/senderUsername at all.
    let header: NotificationHeader = serde_json::from_value(notification.clone())
        .context("decoding VRChat notification type")?;
    let kind = match header.kind.as_str() {
        "invite" => NotificationKind::Invite,
        "requestInvite" => NotificationKind::RequestInvite,
        _ => return Ok(None),
    };

    let wire: WireNotification = serde_json::from_value(notification)
        .context("decoding VRChat invite notification")?;

    Ok(Some(VrchatNotification {
        id: wire.id,
        kind,
        sender_user_id: wire.sender_user_id,
        sender_username: wire.sender_username,
    }))
}

impl VrchatApiClient {
    /// Resolve the two social facts needed by Nightshift's pure invite rules.
    ///
    /// Favorite friends are necessarily friends in VRChat, so finding a favorite lets us avoid
    /// the larger friend-list scan. Otherwise both online and offline friend pages are checked.
    pub async fn invite_sender_relationship(
        &self,
        cookies: &SessionCookies,
        user_id: &str,
    ) -> Result<(bool, bool)> {
        if self.is_favorite_friend(cookies, user_id).await? {
            return Ok((true, true));
        }
        Ok((self.is_friend(cookies, user_id).await?, false))
    }

    async fn is_favorite_friend(&self, cookies: &SessionCookies, user_id: &str) -> Result<bool> {
        let mut offset = 0;
        loop {
            let response = self
                .http
                .get(format!("{BASE_URL}/favorites"))
                .header(COOKIE, cookies.cookie_header())
                .query(&FavoritePageQuery {
                    kind: "friend",
                    n: PAGE_SIZE,
                    offset,
                })
                .send()
                .await
                .context("listing VRChat favorite friends")?;

            if response.status() == StatusCode::UNAUTHORIZED {
                bail!("VRChat session expired while listing favorite friends");
            }
            let response = response
                .error_for_status()
                .context("VRChat favorite-friends request failed")?;
            let page: Vec<FavoriteRecord> = response
                .json()
                .await
                .context("decoding VRChat favorite friends")?;

            if page.iter().any(|favorite| favorite.favorite_id == user_id) {
                return Ok(true);
            }
            if page.len() < PAGE_SIZE {
                return Ok(false);
            }
            offset = offset.saturating_add(page.len());
        }
    }

    async fn is_friend(&self, cookies: &SessionCookies, user_id: &str) -> Result<bool> {
        for offline in [false, true] {
            let mut offset = 0;
            loop {
                let response = self
                    .http
                    .get(format!("{BASE_URL}/auth/user/friends"))
                    .header(COOKIE, cookies.cookie_header())
                    .query(&FriendPageQuery {
                        offline,
                        n: PAGE_SIZE,
                        offset,
                    })
                    .send()
                    .await
                    .context("listing VRChat friends")?;

                if response.status() == StatusCode::UNAUTHORIZED {
                    bail!("VRChat session expired while listing friends");
                }
                let response = response
                    .error_for_status()
                    .context("VRChat friends request failed")?;
                let page: Vec<FriendRecord> = response
                    .json()
                    .await
                    .context("decoding VRChat friends")?;

                if page.iter().any(|friend| friend.id == user_id) {
                    return Ok(true);
                }
                if page.len() < PAGE_SIZE {
                    break;
                }
                offset = offset.saturating_add(page.len());
            }
        }
        Ok(false)
    }

    /// Invite a user to the current instance. VRChat uses this operation to accept an invite request.
    /// A 403 is preserved as InviteRefused so callers can leave the original request visible.
    pub async fn invite_user(
        &self,
        cookies: &SessionCookies,
        user_id: &str,
        instance_id: &str,
        message_slot: Option<u8>,
    ) -> Result<()> {
        let response = self
            .http
            .post(format!("{BASE_URL}/invite/{user_id}"))
            .header(COOKIE, cookies.cookie_header())
            .json(&InviteBody {
                instance_id,
                message_slot,
            })
            .send()
            .await
            .context("sending VRChat invite")?;

        if response.status() == StatusCode::UNAUTHORIZED {
            bail!("VRChat session expired while sending invite");
        }
        if response.status() == StatusCode::FORBIDDEN {
            let body = response.text().await.unwrap_or_default();
            let message = serde_json::from_str::<ApiErrorEnvelope>(&body)
                .ok()
                .and_then(|body| body.error)
                .and_then(|error| error.message);
            return Err(InviteRefused { message, body }.into());
        }
        response
            .error_for_status()
            .context("VRChat invite request failed")?;
        Ok(())
    }

    /// Send a decline response for either an invite or an invite request.
    pub async fn decline_invite_or_request(
        &self,
        cookies: &SessionCookies,
        notification_id: &str,
        response_slot: Option<u8>,
    ) -> Result<()> {
        let response = self
            .http
            .post(format!("{BASE_URL}/invite/{notification_id}/response"))
            .header(COOKIE, cookies.cookie_header())
            .json(&DeclineBody { response_slot })
            .send()
            .await
            .context("declining VRChat invite notification")?;

        if response.status() == StatusCode::UNAUTHORIZED {
            bail!("VRChat session expired while declining invite notification");
        }
        response
            .error_for_status()
            .context("VRChat decline response failed")?;
        Ok(())
    }

    /// Hide a notification after the associated action has succeeded.
    pub async fn hide_notification(
        &self,
        cookies: &SessionCookies,
        notification_id: &str,
    ) -> Result<()> {
        let response = self
            .http
            .put(format!(
                "{BASE_URL}/auth/user/notifications/{notification_id}/hide"
            ))
            .header(COOKIE, cookies.cookie_header())
            .send()
            .await
            .context("hiding VRChat notification")?;

        if response.status() == StatusCode::UNAUTHORIZED {
            bail!("VRChat session expired while hiding notification");
        }
        response
            .error_for_status()
            .context("VRChat notification hide failed")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_url_percent_encodes_auth_token() {
        assert_eq!(
            pipeline_url("abc+/= token"),
            "wss://pipeline.vrchat.cloud/?authToken=abc%2B%2F%3D%20token"
        );
    }

    #[test]
    fn parses_stringified_invite_request_notification() {
        let frame = r#"{
            "type":"notification",
            "content":"{\"id\":\"not_1\",\"type\":\"requestInvite\",\"senderUserId\":\"usr_bob\",\"senderUsername\":\"Bob\"}"
        }"#;
        assert_eq!(
            parse_pipeline_notification(frame).unwrap(),
            Some(VrchatNotification {
                id: "not_1".into(),
                kind: NotificationKind::RequestInvite,
                sender_user_id: "usr_bob".into(),
                sender_username: Some("Bob".into()),
            })
        );
    }

    #[test]
    fn parses_object_invite_notification_without_sender_username() {
        let frame = r#"{
            "type":"notification",
            "content":{
                "id":"not_2",
                "type":"invite",
                "senderUserId":"usr_alice"
            }
        }"#;
        assert_eq!(
            parse_pipeline_notification(frame).unwrap(),
            Some(VrchatNotification {
                id: "not_2".into(),
                kind: NotificationKind::Invite,
                sender_user_id: "usr_alice".into(),
                sender_username: None,
            })
        );
    }

    #[test]
    fn ignores_unrelated_pipeline_events_and_notifications_without_invite_fields() {
        assert_eq!(
            parse_pipeline_notification(r#"{"type":"friend-online","content":"{}"}"#).unwrap(),
            None
        );
        assert_eq!(
            parse_pipeline_notification(
                r#"{"type":"notification","content":"{\"id\":\"not_3\",\"type\":\"friendRequest\"}"}"#
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn social_pages_decode_only_ids_needed_by_policy() {
        let friends: Vec<FriendRecord> = serde_json::from_str(
            r#"[{"id":"usr_friend","displayName":"Friend","status":"active"}]"#,
        )
        .unwrap();
        assert_eq!(friends[0].id, "usr_friend");

        let favorites: Vec<FavoriteRecord> = serde_json::from_str(
            r#"[{"id":"fvrt_1","favoriteId":"usr_favorite","type":"friend","tags":["group_0"]}]"#,
        )
        .unwrap();
        assert_eq!(favorites[0].favorite_id, "usr_favorite");
    }

    #[test]
    fn pagination_queries_use_vrchat_wire_names() {
        assert_eq!(
            serde_json::to_value(FriendPageQuery {
                offline: true,
                n: 100,
                offset: 200,
            })
            .unwrap(),
            serde_json::json!({"offline":true,"n":100,"offset":200})
        );
        assert_eq!(
            serde_json::to_value(FavoritePageQuery {
                kind: "friend",
                n: 100,
                offset: 0,
            })
            .unwrap(),
            serde_json::json!({"type":"friend","n":100,"offset":0})
        );
    }

    #[test]
    fn omits_empty_message_slots_from_request_bodies() {
        assert_eq!(
            serde_json::to_value(InviteBody {
                instance_id: "wrld_test:123",
                message_slot: None,
            })
            .unwrap(),
            serde_json::json!({"instanceId":"wrld_test:123"})
        );
        assert_eq!(
            serde_json::to_value(DeclineBody { response_slot: None }).unwrap(),
            serde_json::json!({})
        );
    }
}
