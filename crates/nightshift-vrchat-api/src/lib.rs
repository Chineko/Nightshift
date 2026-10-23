mod invites;

pub use invites::{
    InviteRefused, NotificationKind, VrchatNotification, parse_pipeline_notification, pipeline_url,
};

use std::{fmt, time::Duration};

use anyhow::{Context, Result, bail};
use nightshift_protocol::VrchatStatus;
use reqwest::{
    Client as HttpClient, StatusCode,
    header::{COOKIE, HeaderMap, SET_COOKIE},
};
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://api.vrchat.cloud/api/1";
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, PartialEq, Eq)]
pub struct SessionCookies {
    pub auth: String,
    pub two_factor_auth: Option<String>,
}

impl fmt::Debug for SessionCookies {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionCookies")
            .field("auth", &"<redacted>")
            .field(
                "two_factor_auth",
                &self.two_factor_auth.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TwoFactorMethod {
    Totp,
    EmailOtp,
}

impl TwoFactorMethod {
    fn path_component(self) -> &'static str {
        match self {
            Self::Totp => "totp",
            Self::EmailOtp => "emailotp",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingTwoFactor {
    pub method: TwoFactorMethod,
    pub cookies: SessionCookies,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedSession {
    pub user: CurrentUserSummary,
    pub cookies: SessionCookies,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginResult {
    Authenticated(AuthenticatedSession),
    TwoFactorRequired(PendingTwoFactor),
}

#[derive(Debug, Deserialize)]
struct CurrentUserResponse {
    id: String,
    #[serde(rename = "displayName")]
    display_name: String,
    status: String,
}

#[derive(Debug, Default, Deserialize)]
struct LoginResponse {
    id: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    status: Option<String>,
    #[serde(rename = "requiresTwoFactorAuth", default)]
    requires_two_factor_auth: Vec<String>,
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VerifyTwoFactorResponse {
    verified: Option<bool>,
}

#[derive(Serialize)]
struct VerifyTwoFactorBody<'a> {
    code: &'a str,
}

#[derive(Debug, Serialize)]
struct StatusChangeBody<'a> {
    status: &'a str,
}

#[derive(Default, PartialEq, Eq)]
struct CapturedCookies {
    auth: Option<String>,
    two_factor_auth: Option<String>,
}

impl fmt::Debug for CapturedCookies {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedCookies")
            .field("auth", &self.auth.as_ref().map(|_| "<redacted>"))
            .field(
                "two_factor_auth",
                &self.two_factor_auth.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
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
            .connect_timeout(HTTP_CONNECT_TIMEOUT)
            .timeout(HTTP_REQUEST_TIMEOUT)
            .build()
            .context("building VRChat HTTP client")?;
        Ok(Self { http })
    }

    pub async fn begin_login(&self, username: &str, password: &str) -> Result<LoginResult> {
        // VRChat's established client behavior percent-encodes both Basic-auth fields first.
        let username = urlencoding::encode(username).into_owned();
        let password = urlencoding::encode(password).into_owned();
        let response = self
            .http
            .get(format!("{BASE_URL}/auth/user"))
            .basic_auth(username, Some(password))
            .send()
            .await
            .context("starting VRChat login")?;

        let captured = capture_response_cookies(response.headers())?;
        let status = response.status();
        let body: LoginResponse = response.json().await.unwrap_or_default();

        if status == StatusCode::UNAUTHORIZED {
            let detail = body
                .error
                .and_then(|error| error.message)
                .unwrap_or_else(|| "VRChat rejected the login".into());
            bail!("{detail}");
        }
        if !status.is_success() {
            bail!("VRChat login failed with HTTP {status}");
        }

        let auth = captured
            .auth
            .context("VRChat login response did not provide an auth session cookie")?;
        let cookies = SessionCookies {
            auth,
            two_factor_auth: captured.two_factor_auth,
        };

        if let Some(method) = preferred_two_factor_method(&body.requires_two_factor_auth) {
            return Ok(LoginResult::TwoFactorRequired(PendingTwoFactor {
                method,
                cookies,
            }));
        }
        if !body.requires_two_factor_auth.is_empty() {
            bail!(
                "VRChat requires an unsupported two-factor method: {:?}",
                body.requires_two_factor_auth
            );
        }

        Ok(LoginResult::Authenticated(AuthenticatedSession {
            user: login_user(body)?,
            cookies,
        }))
    }

    pub async fn verify_two_factor(
        &self,
        pending: PendingTwoFactor,
        code: &str,
    ) -> Result<AuthenticatedSession> {
        let response = self
            .http
            .post(format!(
                "{BASE_URL}/auth/twofactorauth/{}/verify",
                pending.method.path_component()
            ))
            .header(COOKIE, pending.cookies.cookie_header())
            .json(&VerifyTwoFactorBody { code })
            .send()
            .await
            .context("verifying VRChat two-factor code")?;

        let captured = capture_response_cookies(response.headers())?;
        let status = response.status();
        let body: VerifyTwoFactorResponse = response
            .json()
            .await
            .unwrap_or(VerifyTwoFactorResponse { verified: None });

        if status == StatusCode::UNAUTHORIZED || body.verified == Some(false) {
            bail!("VRChat rejected the two-factor code");
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            bail!("VRChat rate-limited the two-factor verification");
        }
        if !status.is_success() || body.verified != Some(true) {
            bail!("VRChat returned an unexpected two-factor response");
        }

        let cookies = SessionCookies {
            auth: captured.auth.unwrap_or(pending.cookies.auth),
            two_factor_auth: captured
                .two_factor_auth
                .or(pending.cookies.two_factor_auth),
        };
        if cookies.two_factor_auth.is_none() {
            bail!("VRChat verified two-factor authentication without providing a session cookie");
        }
        let user = self.current_user(&cookies).await?;
        Ok(AuthenticatedSession { user, cookies })
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

fn login_user(body: LoginResponse) -> Result<CurrentUserSummary> {
    Ok(CurrentUserSummary {
        id: body.id.context("VRChat login response is missing user id")?,
        display_name: body
            .display_name
            .context("VRChat login response is missing display name")?,
        status: parse_status(
            body.status
                .as_deref()
                .context("VRChat login response is missing status")?,
        )?,
    })
}

fn preferred_two_factor_method(methods: &[String]) -> Option<TwoFactorMethod> {
    if methods.iter().any(|method| method == "totp") {
        Some(TwoFactorMethod::Totp)
    } else if methods.iter().any(|method| method == "emailotp") {
        Some(TwoFactorMethod::EmailOtp)
    } else {
        None
    }
}

fn capture_response_cookies(headers: &HeaderMap) -> Result<CapturedCookies> {
    let mut captured = CapturedCookies::default();
    for value in headers.get_all(SET_COOKIE).iter() {
        let value = value
            .to_str()
            .context("VRChat returned a non-text Set-Cookie header")?;
        let Some((name, value)) = parse_set_cookie(value) else {
            continue;
        };
        match name {
            "auth" => captured.auth = Some(value.to_string()),
            "twoFactorAuth" => captured.two_factor_auth = Some(value.to_string()),
            _ => {}
        }
    }
    Ok(captured)
}

fn parse_set_cookie(value: &str) -> Option<(&str, &str)> {
    let pair = value.split(';').next()?.trim();
    let (name, value) = pair.split_once('=')?;
    Some((name.trim(), value.trim()))
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
    use reqwest::header::HeaderValue;

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
    fn secret_cookie_debug_output_is_redacted() {
        let cookies = SessionCookies {
            auth: "auth-super-secret".into(),
            two_factor_auth: Some("2fa-super-secret".into()),
        };
        let debug = format!("{cookies:?}");
        assert!(!debug.contains("auth-super-secret"));
        assert!(!debug.contains("2fa-super-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn captures_only_vrchat_session_cookies() {
        let mut headers = HeaderMap::new();
        headers.append(SET_COOKIE, HeaderValue::from_static("auth=abc==; Path=/; HttpOnly"));
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("twoFactorAuth=def; Path=/; HttpOnly"),
        );
        headers.append(SET_COOKIE, HeaderValue::from_static("unrelated=nope; Path=/"));
        assert_eq!(
            capture_response_cookies(&headers).unwrap(),
            CapturedCookies {
                auth: Some("abc==".into()),
                two_factor_auth: Some("def".into()),
            }
        );
    }

    #[test]
    fn prefers_totp_then_email_otp() {
        assert_eq!(
            preferred_two_factor_method(&["emailotp".into(), "totp".into()]),
            Some(TwoFactorMethod::Totp)
        );
        assert_eq!(
            preferred_two_factor_method(&["emailotp".into()]),
            Some(TwoFactorMethod::EmailOtp)
        );
        assert_eq!(preferred_two_factor_method(&["webauthn".into()]), None);
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
