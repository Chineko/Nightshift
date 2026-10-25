use std::{collections::HashSet, error::Error, fmt, time::Instant};

use anyhow::{Context, Result, bail};
use reqwest::{StatusCode, header::COOKIE};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    BASE_URL, FAVORITES_CACHE_TTL, FavoriteCache, SessionCookies, VrchatApiClient,
    api_status_error, validate_vrchat_user_id,
};

const PIPELINE_URL: &str = "wss://pipeline.vrchat.cloud/";
const PAGE_SIZE: usize = 100;
const MAX_FAVORITE_PAGES: usize = 100;

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
struct FriendStatusResponse {
    #[serde(rename = "isFriend")]
    is_friend: bool,
}

#[derive(Debug, Deserialize)]
struct FavoriteRecord {
    #[serde(rename = "favoriteId")]
    favorite_id: String,
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
    /// Resolve both social facts needed by callers that need friend and favorite context together.
    pub async fn invite_sender_relationship(
        &self,
        cookies: &SessionCookies,
        user_id: &str,
    ) -> Result<(bool, bool)> {
        validate_vrchat_user_id(user_id)?;
        if self.invite_sender_is_favorite(cookies, user_id).await? {
            return Ok((true, true));
        }
        Ok((self.invite_sender_is_friend(cookies, user_id).await?, false))
    }

    /// Check only whether the invite sender is a favorite friend.
    ///
    /// Favorite membership is cached briefly per authenticated VRChat session so a burst of invite
    /// notifications does not repeatedly page through the complete favorites collection.
    pub async fn invite_sender_is_favorite(
        &self,
        cookies: &SessionCookies,
        user_id: &str,
    ) -> Result<bool> {
        let user_id = validate_vrchat_user_id(user_id)?;
        if let Some(is_favorite) = self.cached_favorite_contains(cookies, user_id) {
            return Ok(is_favorite);
        }

        let user_ids = self.fetch_favorite_friend_ids(cookies).await?;
        let is_favorite = user_ids.contains(user_id);
        let mut cache = self
            .favorite_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *cache = Some(FavoriteCache {
            session_auth: cookies.auth.clone(),
            loaded_at: Instant::now(),
            user_ids,
        });
        Ok(is_favorite)
    }

    fn cached_favorite_contains(
        &self,
        cookies: &SessionCookies,
        user_id: &str,
    ) -> Option<bool> {
        let cache = self
            .favorite_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let cache = cache.as_ref()?;
        if cache.session_auth != cookies.auth || cache.loaded_at.elapsed() > FAVORITES_CACHE_TTL {
            return None;
        }
        Some(cache.user_ids.contains(user_id))
    }

    async fn fetch_favorite_friend_ids(
        &self,
        cookies: &SessionCookies,
    ) -> Result<HashSet<String>> {
        let mut user_ids = HashSet::new();
        let mut offset = 0;
        for _ in 0..MAX_FAVORITE_PAGES {
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

            if let Some(error) = api_status_error(response.status(), "listing favorite friends") {
                return Err(error.into());
            }
            let response = response
                .error_for_status()
                .context("VRChat favorite-friends request failed")?;
            let page: Vec<FavoriteRecord> = response
                .json()
                .await
                .context("decoding VRChat favorite friends")?;
            let page_len = page.len();
            user_ids.extend(page.into_iter().map(|favorite| favorite.favorite_id));

            if page_len < PAGE_SIZE {
                return Ok(user_ids);
            }
            offset = offset
                .checked_add(page_len)
                .context("VRChat favorite-friends pagination offset overflow")?;
        }

        bail!("VRChat favorite-friends pagination exceeded safety limit")
    }

    /// Check only whether the invite sender is a friend using VRChat's targeted endpoint.
    pub async fn invite_sender_is_friend(
        &self,
        cookies: &SessionCookies,
        user_id: &str,
    ) -> Result<bool> {
        let user_id = validate_vrchat_user_id(user_id)?;
        let response = self
            .http
            .get(format!("{BASE_URL}/user/{user_id}/friendStatus"))
            .header(COOKIE, cookies.cookie_header())
            .send()
            .await
            .context("checking VRChat invite sender friend status")?;

        if let Some(error) = api_status_error(response.status(), "checking friend status") {
            return Err(error.into());
        }
        let response = response
            .error_for_status()
            .context("VRChat friend-status request failed")?;
        let status: FriendStatusResponse = response
            .json()
            .await
            .context("decoding VRChat friend status")?;
        Ok(status.is_friend)
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
        let user_id = validate_vrchat_user_id(user_id)?;
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

        if let Some(error) = api_status_error(response.status(), "sending invite") {
            return Err(error.into());
        }
        if response.status() == StatusCode::FORBIDDEN {
            // 403 is VRChat saying "nope" to this invite, not a dead session. Keep the evidence.
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

        if let Some(error) = api_status_error(response.status(), "declining invite notification") {
            return Err(error.into());
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

        if let Some(error) = api_status_error(response.status(), "hiding notification") {
            return Err(error.into());
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
    fn social_responses_decode_only_fields_needed_by_policy() {
        let friend: FriendStatusResponse = serde_json::from_str(
            r#"{"incomingRequest":false,"isFriend":true,"outgoingRequest":false}"#,
        )
        .unwrap();
        assert!(friend.is_friend);

        let favorites: Vec<FavoriteRecord> = serde_json::from_str(
            r#"[{"id":"fvrt_1","favoriteId":"usr_favorite","type":"friend","tags":["group_0"]}]"#,
        )
        .unwrap();
        assert_eq!(favorites[0].favorite_id, "usr_favorite");
    }

    #[test]
    fn favorite_pagination_query_uses_vrchat_wire_names() {
        assert_eq!(
            serde_json::to_value(FavoritePageQuery {
                kind: "friend",
                n: 100,
                offset: 0,
            })
            .unwrap(),
            serde_json::json!({"type":"friend","n":100,"offset":0})
        );
        assert_eq!(MAX_FAVORITE_PAGES, 100);
    }

    #[test]
    fn favorite_cache_is_session_scoped_and_short_lived() {
        let client = VrchatApiClient::new().unwrap();
        let cookies = SessionCookies {
            auth: "session-a".into(),
            two_factor_auth: None,
        };
        let other_cookies = SessionCookies {
            auth: "session-b".into(),
            two_factor_auth: None,
        };
        let user_id = "usr_01234567-89ab-cdef-0123-456789abcdef";
        let mut ids = HashSet::new();
        ids.insert(user_id.to_string());
        *client
            .favorite_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(FavoriteCache {
            session_auth: cookies.auth.clone(),
            loaded_at: Instant::now(),
            user_ids: ids,
        });

        assert_eq!(client.cached_favorite_contains(&cookies, user_id), Some(true));
        assert_eq!(
            client.cached_favorite_contains(&other_cookies, user_id),
            None
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
