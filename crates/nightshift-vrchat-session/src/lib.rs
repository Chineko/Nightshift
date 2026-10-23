use anyhow::{Context, Result};
use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};

const SERVICE: &str = "io.github.chineko.Nightshift.vrchat";
const DEFAULT_PROFILE: &str = "active";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VrchatSession {
    pub auth_cookie: String,
    #[serde(default)]
    pub two_factor_cookie: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

impl VrchatSession {
    pub fn new(auth_cookie: impl Into<String>) -> Self {
        Self {
            auth_cookie: auth_cookie.into(),
            two_factor_cookie: None,
            user_id: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VrchatSessionStore {
    profile: String,
}

impl Default for VrchatSessionStore {
    fn default() -> Self {
        Self::new(DEFAULT_PROFILE)
    }
}

impl VrchatSessionStore {
    pub fn new(profile: impl Into<String>) -> Self {
        Self {
            profile: normalize_profile(profile.into()),
        }
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    pub fn save(&self, session: &VrchatSession) -> Result<()> {
        let secret = serde_json::to_vec(session).context("serializing VRChat session")?;
        self.entry()?
            .set_secret(&secret)
            .context("saving VRChat session to the desktop keyring")
    }

    pub fn load(&self) -> Result<Option<VrchatSession>> {
        let secret = match self.entry()?.get_secret() {
            Ok(secret) => secret,
            Err(KeyringError::NoEntry) => return Ok(None),
            Err(error) => {
                return Err(error).context("reading VRChat session from the desktop keyring");
            }
        };

        let session = serde_json::from_slice(&secret)
            .context("decoding VRChat session stored in the desktop keyring")?;
        Ok(Some(session))
    }

    pub fn clear(&self) -> Result<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(error).context("removing VRChat session from the desktop keyring"),
        }
    }

    fn entry(&self) -> Result<Entry> {
        Entry::new(SERVICE, &self.profile).context("opening Nightshift VRChat keyring entry")
    }
}

fn normalize_profile(profile: String) -> String {
    let profile = profile.trim();
    if profile.is_empty() {
        DEFAULT_PROFILE.to_string()
    } else {
        profile.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_payload_round_trips_without_a_keyring() {
        let session = VrchatSession {
            auth_cookie: "auth-token".into(),
            two_factor_cookie: Some("2fa-token".into()),
            user_id: Some("usr_test".into()),
        };

        let bytes = serde_json::to_vec(&session).unwrap();
        let decoded: VrchatSession = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, session);
    }

    #[test]
    fn older_session_payload_without_optional_fields_still_decodes() {
        let decoded: VrchatSession =
            serde_json::from_str(r#"{"auth_cookie":"auth-token"}"#).unwrap();
        assert_eq!(decoded.auth_cookie, "auth-token");
        assert_eq!(decoded.two_factor_cookie, None);
        assert_eq!(decoded.user_id, None);
    }

    #[test]
    fn blank_profile_uses_active_slot() {
        assert_eq!(VrchatSessionStore::new("   ").profile(), DEFAULT_PROFILE);
        assert_eq!(VrchatSessionStore::default().profile(), DEFAULT_PROFILE);
    }
}
