use anyhow::{Context, Result, bail};
use nightshift_protocol::VrchatStatus;
use reqwest::{Client as HttpClient, StatusCode, header::COOKIE};
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://api.vrchat.cloud/api/1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionCookies {
    pub auth: String,
    pub two_factor_auth: Option<String>,
}

impl SessionCookies {
    pub fn cookie_header(&self) -> String {
        let mut value = format!("auth={}", self.auth);
        if let Some(two_factor_auth) = &self.two_factor_auth {
            value.push_str("; twoFactorAuth=");
            value.push_str(two_factor_auth);
        }
        value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentUserSummary {
    pub id: String,
    pub display_name: String,
    pub status: VrchatStatus,
}

#[derive(Debug, Deserialize)]
struct CurrentUserResponse {
    id: String,
    #[serde(rename = "displayName")]
    display_name: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct StatusChangeBody<'a> {
    status: &'a str,
}

#[derive(Clone)]
pub struct VrchatApiClient {
    http: HttpClient,
}

impl VrchatApiClient {
    pub fn new() -> Result<Self> {
        let http = HttpClient::builder()
            .user_agent(format!(
                "Nightshift/{} (https://github.com/Chineko/Nightshift)",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .context("building VRChat HTTP client")?;
        Ok(Self { http })
    }

    pub async fn current_user(&self, cookies: &SessionCookies) -> Result<CurrentUserSummary> {
        let response = self
            .http
            .get(format!("{BASE_URL}/auth/user"))
            .header(COOKIE, cookies.cookie_header())
            .send()
            .await
            .context("requesting VRChat current user")?;

        if response.status() == StatusCode::UNAUTHORIZED {
            bail!("VRChat session is not authenticated");
        }
        let response = response
            .error_for_status()
            .context("VRChat current-user request failed")?;
        let user: CurrentUserResponse = response
            .json()
            .await
            .context("decoding VRChat current user")?;

        Ok(CurrentUserSummary {
            id: user.id,
            display_name: user.display_name,
            status: parse_status(&user.status)?,
        })
    }

    pub async fn set_status(
        &self,
        cookies: &SessionCookies,
        user_id: &str,
        status: VrchatStatus,
    ) -> Result<()> {
        let response = self
            .http
            .put(format!("{BASE_URL}/users/{user_id}"))
            .header(COOKIE, cookies.cookie_header())
            .json(&StatusChangeBody {
                status: status_wire_value(status),
            })
            .send()
            .await
            .context("updating VRChat status")?;

        if response.status() == StatusCode::UNAUTHORIZED {
            bail!("VRChat session expired while updating status");
        }
        response
            .error_for_status()
            .context("VRChat status update failed")?;
        Ok(())
    }
}

impl Default for VrchatApiClient {
    fn default() -> Self {
        Self::new().expect("building the default VRChat HTTP client should succeed")
    }
}

pub fn status_wire_value(status: VrchatStatus) -> &'static str {
    match status {
        VrchatStatus::Active => "active",
        VrchatStatus::JoinMe => "join me",
        VrchatStatus::AskMe => "ask me",
        VrchatStatus::Busy => "busy",
        VrchatStatus::Offline => "offline",
    }
}

pub fn parse_status(value: &str) -> Result<VrchatStatus> {
    match value {
        "active" => Ok(VrchatStatus::Active),
        "join me" => Ok(VrchatStatus::JoinMe),
        "ask me" => Ok(VrchatStatus::AskMe),
        "busy" => Ok(VrchatStatus::Busy),
        "offline" => Ok(VrchatStatus::Offline),
        other => bail!("unknown VRChat status {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_header_uses_vrchat_cookie_names() {
        assert_eq!(
            SessionCookies {
                auth: "abc".into(),
                two_factor_auth: Some("def".into()),
            }
            .cookie_header(),
            "auth=abc; twoFactorAuth=def"
        );
        assert_eq!(
            SessionCookies {
                auth: "abc".into(),
                two_factor_auth: None,
            }
            .cookie_header(),
            "auth=abc"
        );
    }

    #[test]
    fn status_values_match_vrchat_wire_format() {
        let cases = [
            (VrchatStatus::Active, "active"),
            (VrchatStatus::JoinMe, "join me"),
            (VrchatStatus::AskMe, "ask me"),
            (VrchatStatus::Busy, "busy"),
            (VrchatStatus::Offline, "offline"),
        ];
        for (status, wire) in cases {
            assert_eq!(status_wire_value(status), wire);
            assert_eq!(parse_status(wire).unwrap(), status);
        }
    }

    #[test]
    fn rejects_unknown_statuses() {
        assert!(parse_status("creative chaos").is_err());
    }
}
