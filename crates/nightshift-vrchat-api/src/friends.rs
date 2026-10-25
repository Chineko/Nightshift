#[path = "friends/images.rs"]
mod images;

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use reqwest::header::COOKIE;
use serde::Deserialize;

use super::{BASE_URL, SessionCookies, VrchatApiClient, api_status_error};

pub const FRIEND_METADATA_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
const FRIEND_PAGE_SIZE: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FriendProfile {
    pub id: String,
    pub display_name: String,
    pub icon_url: Option<String>,
    pub profile_pic_override: Option<String>,
}

impl FriendProfile {
    pub fn preferred_image_url(&self) -> Option<&str> {
        self.icon_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.profile_pic_override
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
    }
}

#[derive(Clone, Debug, Deserialize)]
struct FriendResponse {
    id: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "iconUrl", default)]
    icon_url: Option<String>,
    #[serde(rename = "profilePicOverride", default)]
    profile_pic_override: Option<String>,
}

impl From<FriendResponse> for FriendProfile {
    fn from(value: FriendResponse) -> Self {
        Self {
            id: value.id,
            display_name: value.display_name,
            icon_url: normalize_optional_url(value.icon_url),
            profile_pic_override: normalize_optional_url(value.profile_pic_override),
        }
    }
}

pub(crate) struct FriendCache {
    pub(crate) session_auth: String,
    pub(crate) loaded_at: Instant,
    pub(crate) profiles: Vec<FriendProfile>,
}

impl FriendCache {
    pub(crate) fn matches_session(&self, cookies: &SessionCookies) -> bool {
        self.session_auth == cookies.auth
    }

    pub(crate) fn fresh_for(&self, cookies: &SessionCookies, now: Instant) -> bool {
        self.matches_session(cookies)
            && now.saturating_duration_since(self.loaded_at) < FRIEND_METADATA_CACHE_TTL
    }
}

impl VrchatApiClient {
    pub async fn friends(&self, cookies: &SessionCookies) -> Result<Vec<FriendProfile>> {
        let now = Instant::now();
        if let Ok(cache) = self.friend_cache.lock()
            && let Some(cache) = cache.as_ref()
            && cache.fresh_for(cookies, now)
        {
            return Ok(cache.profiles.clone());
        }

        let profiles = self.fetch_all_friends(cookies).await?;
        if let Ok(mut cache) = self.friend_cache.lock() {
            *cache = Some(FriendCache {
                session_auth: cookies.auth.clone(),
                loaded_at: Instant::now(),
                profiles: profiles.clone(),
            });
        }
        Ok(profiles)
    }

    pub fn cached_friends(&self, cookies: &SessionCookies) -> Option<Vec<FriendProfile>> {
        let cache = self.friend_cache.lock().ok()?;
        let cache = cache.as_ref()?;
        cache.matches_session(cookies).then(|| cache.profiles.clone())
    }

    pub fn friend_cache_age(&self, cookies: &SessionCookies) -> Option<Duration> {
        let now = Instant::now();
        let cache = self.friend_cache.lock().ok()?;
        let cache = cache.as_ref()?;
        cache
            .matches_session(cookies)
            .then(|| now.saturating_duration_since(cache.loaded_at))
    }

    pub fn invalidate_friend_cache(&self) {
        if let Ok(mut cache) = self.friend_cache.lock() {
            *cache = None;
        }
    }

    async fn fetch_all_friends(&self, cookies: &SessionCookies) -> Result<Vec<FriendProfile>> {
        let mut profiles = HashMap::<String, FriendProfile>::new();

        for offline in [false, true] {
            let mut offset = 0usize;
            loop {
                let response = self
                    .http
                    .get(format!("{BASE_URL}/auth/user/friends"))
                    .header(COOKIE, cookies.cookie_header())
                    .query(&[
                        ("n", FRIEND_PAGE_SIZE.to_string()),
                        ("offset", offset.to_string()),
                        ("offline", offline.to_string()),
                    ])
                    .send()
                    .await
                    .context("requesting VRChat friends")?;

                if let Some(error) = api_status_error(response.status(), "refreshing friends") {
                    return Err(error.into());
                }

                let response = response
                    .error_for_status()
                    .context("VRChat friend refresh failed")?;
                let page: Vec<FriendResponse> = response
                    .json()
                    .await
                    .context("decoding VRChat friend list")?;
                let page_len = page.len();

                for friend in page {
                    let friend = FriendProfile::from(friend);
                    profiles.insert(friend.id.clone(), friend);
                }

                if page_len < FRIEND_PAGE_SIZE {
                    break;
                }
                offset += page_len;
            }
        }

        let mut profiles = profiles.into_values().collect::<Vec<_>>();
        profiles.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(profiles)
    }
}

fn normalize_optional_url(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_friend_identity_and_image_metadata() {
        let response: FriendResponse = serde_json::from_str(
            r#"{
                "id":"usr_01234567-89ab-cdef-0123-456789abcdef",
                "displayName":"Sleepy Friend",
                "iconUrl":"https://api.vrchat.cloud/api/1/file/file_icon/1/file",
                "profilePicOverride":"https://api.vrchat.cloud/api/1/file/file_profile/1/file"
            }"#,
        )
        .unwrap();
        let profile = FriendProfile::from(response);
        assert_eq!(profile.display_name, "Sleepy Friend");
        assert_eq!(
            profile.preferred_image_url(),
            Some("https://api.vrchat.cloud/api/1/file/file_icon/1/file")
        );
    }

    #[test]
    fn falls_back_to_profile_picture_when_icon_is_missing() {
        let profile = FriendProfile {
            id: "usr_01234567-89ab-cdef-0123-456789abcdef".into(),
            display_name: "Friend".into(),
            icon_url: Some("   ".into()),
            profile_pic_override: Some("https://example.com/profile.png".into()),
        };
        assert_eq!(profile.preferred_image_url(), Some("https://example.com/profile.png"));
    }

    #[test]
    fn cache_is_bound_to_session_and_ttl() {
        let cookies = SessionCookies {
            auth: "session-a".into(),
            two_factor_auth: None,
        };
        let loaded_at = Instant::now();
        let cache = FriendCache {
            session_auth: cookies.auth.clone(),
            loaded_at,
            profiles: Vec::new(),
        };
        assert!(cache.fresh_for(&cookies, loaded_at + Duration::from_secs(3599)));
        assert!(!cache.fresh_for(&cookies, loaded_at + Duration::from_secs(3600)));
        assert!(!cache.fresh_for(
            &SessionCookies {
                auth: "session-b".into(),
                two_factor_auth: None,
            },
            loaded_at + Duration::from_secs(1),
        ));
    }

    #[test]
    fn stale_cache_stays_readable_only_within_same_session() {
        let cookies = SessionCookies {
            auth: "session-a".into(),
            two_factor_auth: None,
        };
        let cache = FriendCache {
            session_auth: cookies.auth.clone(),
            loaded_at: Instant::now() - FRIEND_METADATA_CACHE_TTL - Duration::from_secs(1),
            profiles: vec![FriendProfile {
                id: "usr_friend".into(),
                display_name: "Friend".into(),
                icon_url: None,
                profile_pic_override: None,
            }],
        };

        assert!(cache.matches_session(&cookies));
        assert!(!cache.fresh_for(&cookies, Instant::now()));
        assert!(!cache.matches_session(&SessionCookies {
            auth: "session-b".into(),
            two_factor_auth: None,
        }));
    }

    #[test]
    fn empty_image_fields_normalize_to_none() {
        assert_eq!(normalize_optional_url(None), None);
        assert_eq!(normalize_optional_url(Some("".into())), None);
        assert_eq!(normalize_optional_url(Some("  ".into())), None);
        assert_eq!(
            normalize_optional_url(Some(" https://example.com/icon.png ".into())),
            Some("https://example.com/icon.png".into())
        );
    }
}
