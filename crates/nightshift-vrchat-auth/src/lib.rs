use std::sync::Arc;

use anyhow::{Context, Result, bail};
pub use nightshift_vrchat_api::FriendProfile;
use nightshift_vrchat_api::{
    AuthenticatedSession, CurrentUserSummary, LoginResult, PendingTwoFactor, SessionCookies,
    TwoFactorMethod, VrchatApiClient,
};
use nightshift_vrchat_session::{VrchatSession, VrchatSessionStore};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingAuth {
    inner: PendingTwoFactor,
}

impl PendingAuth {
    pub fn method_name(&self) -> &'static str {
        match self.inner.method {
            TwoFactorMethod::Totp => "Authenticator code",
            TwoFactorMethod::EmailOtp => "Email code",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthStep {
    Authenticated(CurrentUserSummary),
    TwoFactorRequired(PendingAuth),
}

#[derive(Clone)]
pub struct VrchatAuthService {
    api: VrchatApiClient,
    store: VrchatSessionStore,
}

impl VrchatAuthService {
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: VrchatApiClient::new()?,
            store: VrchatSessionStore::default(),
        })
    }

    pub fn with_store(store: VrchatSessionStore) -> Result<Self> {
        Ok(Self {
            api: VrchatApiClient::new()?,
            store,
        })
    }

    pub async fn begin_login(&self, username: &str, password: &str) -> Result<AuthStep> {
        match self.api.begin_login(username, password).await? {
            LoginResult::Authenticated(session) => {
                let user = session.user.clone();
                self.persist(session).await?;
                Ok(AuthStep::Authenticated(user))
            }
            LoginResult::TwoFactorRequired(pending) => {
                Ok(AuthStep::TwoFactorRequired(PendingAuth { inner: pending }))
            }
        }
    }

    pub async fn verify_two_factor(
        &self,
        pending: &PendingAuth,
        code: &str,
    ) -> Result<CurrentUserSummary> {
        let session = self.api.verify_two_factor(pending.inner.clone(), code).await?;
        let user = session.user.clone();
        self.persist(session).await?;
        Ok(user)
    }

    pub async fn friends(&self) -> Result<Vec<FriendProfile>> {
        let cookies = self.stored_cookies().await?;
        self.api.friends(&cookies).await
    }

    pub async fn friend_image_bytes(&self, url: &str) -> Result<Arc<[u8]>> {
        self.api.friend_image_bytes(url).await
    }

    pub async fn clear_session(&self) -> Result<()> {
        self.api.invalidate_friend_cache();
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.clear())
            .await
            .context("VRChat keyring clear task stopped")?
    }

    async fn stored_cookies(&self) -> Result<SessionCookies> {
        let store = self.store.clone();
        let session = tokio::task::spawn_blocking(move || store.load())
            .await
            .context("VRChat keyring load task stopped")??
            .context("No stored VRChat session. Connect your account first.")?;
        if session.auth_cookie.trim().is_empty() {
            bail!("Stored VRChat session is missing its auth cookie");
        }
        Ok(SessionCookies {
            auth: session.auth_cookie,
            two_factor_auth: session.two_factor_cookie,
        })
    }

    async fn persist(&self, session: AuthenticatedSession) -> Result<()> {
        self.api.invalidate_friend_cache();
        let stored = stored_session(session);
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.save(&stored))
            .await
            .context("VRChat keyring save task stopped")?
    }
}

impl Default for VrchatAuthService {
    fn default() -> Self {
        Self::new().expect("building the default VRChat auth service should succeed")
    }
}

fn stored_session(session: AuthenticatedSession) -> VrchatSession {
    VrchatSession {
        auth_cookie: session.cookies.auth,
        two_factor_cookie: session.cookies.two_factor_auth,
        user_id: Some(session.user.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nightshift_protocol::VrchatStatus;

    #[test]
    fn authenticated_result_becomes_keyring_session_without_credentials() {
        let stored = stored_session(AuthenticatedSession {
            user: CurrentUserSummary {
                id: "usr_test".into(),
                display_name: "Hydra Employee".into(),
                status: VrchatStatus::Active,
            },
            cookies: SessionCookies {
                auth: "auth-cookie".into(),
                two_factor_auth: Some("2fa-cookie".into()),
            },
        });

        assert_eq!(stored.auth_cookie, "auth-cookie");
        assert_eq!(stored.two_factor_cookie.as_deref(), Some("2fa-cookie"));
        assert_eq!(stored.user_id.as_deref(), Some("usr_test"));
    }

    #[test]
    fn pending_auth_exposes_method_but_not_session_cookie_fields() {
        let pending = PendingAuth {
            inner: PendingTwoFactor {
                method: TwoFactorMethod::Totp,
                cookies: SessionCookies {
                    auth: "secret".into(),
                    two_factor_auth: None,
                },
            },
        };
        assert_eq!(pending.method_name(), "Authenticator code");
    }
}
