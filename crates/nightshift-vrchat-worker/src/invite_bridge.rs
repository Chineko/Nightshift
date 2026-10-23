use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use nightshift_invites::{InviteContext, InviteRules};
use nightshift_protocol::{InviteProfile, InviteState};
use nightshift_vrchat_api::{NotificationKind, SessionCookies, VrchatApiClient, VrchatNotification};

use crate::{
    instance_context::InstanceSource,
    invite_execute::{InviteExecutionOutcome, execute_invite_plan},
    invite_plan::plan_invite_action,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InviteHandlingOutcome {
    Execution(InviteExecutionOutcome),
    /// These profiles require persisted user-supplied rule data that is not part of protocol v1 yet.
    ConfigurationUnavailable(InviteProfile),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InviteHandlingResult {
    pub notification_id: String,
    pub sender_user_id: String,
    pub sender_username: Option<String>,
    pub outcome: InviteHandlingOutcome,
}

pub fn invite_rules_from_state(state: &InviteState) -> Result<InviteRules, InviteProfile> {
    if !state.enabled || state.profile == InviteProfile::Disabled {
        return Ok(InviteRules {
            enabled: false,
            profile: state.profile,
            ..Default::default()
        });
    }

    match state.profile {
        InviteProfile::Friends | InviteProfile::Favorites => Ok(InviteRules {
            enabled: true,
            profile: state.profile,
            ..Default::default()
        }),
        InviteProfile::Disabled => unreachable!("disabled profile handled above"),
        InviteProfile::Whitelist | InviteProfile::Custom => Err(state.profile),
    }
}

pub async fn handle_invite_notification(
    api: VrchatApiClient,
    cookies: SessionCookies,
    invite_state: InviteState,
    instance_source: Arc<Mutex<InstanceSource>>,
    notification: VrchatNotification,
) -> Result<InviteHandlingResult> {
    let base = |outcome| InviteHandlingResult {
        notification_id: notification.id.clone(),
        sender_user_id: notification.sender_user_id.clone(),
        sender_username: notification.sender_username.clone(),
        outcome,
    };

    let rules = match invite_rules_from_state(&invite_state) {
        Ok(rules) => rules,
        Err(profile) => {
            return Ok(base(InviteHandlingOutcome::ConfigurationUnavailable(profile)));
        }
    };

    if !rules.enabled {
        return Ok(base(InviteHandlingOutcome::Execution(
            InviteExecutionOutcome::None,
        )));
    }

    let (is_friend, is_favorite) = api
        .invite_sender_relationship(&cookies, &notification.sender_user_id)
        .await
        .context("resolving VRChat invite sender relationship")?;

    let decision = rules.decide(&InviteContext {
        user_id: notification.sender_user_id.clone(),
        is_friend,
        is_favorite,
        // User-defined whitelist data is intentionally unavailable until the invite settings
        // backend/UI adds it. Do not infer whitelist membership from other relationship facts.
        is_whitelisted: false,
    });

    let current_instance = if decision == nightshift_protocol::InviteActionKind::Accepted
        && notification.kind == NotificationKind::RequestInvite
    {
        tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let mut source = instance_source
                .lock()
                .map_err(|_| anyhow!("VRChat instance source lock was poisoned"))?;
            source
                .poll()
                .context("reading current VRChat instance from local log")
        })
        .await
        .context("VRChat instance-context task stopped")??
    } else {
        None
    };

    let plan = plan_invite_action(&notification, decision, current_instance.as_deref());
    let outcome = execute_invite_plan(&api, &cookies, plan)
        .await
        .context("executing VRChat invite automation action")?;

    Ok(base(InviteHandlingOutcome::Execution(outcome)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_policy_stays_disabled() {
        let rules = invite_rules_from_state(&InviteState::default()).unwrap();
        assert!(!rules.enabled);
        assert_eq!(rules.profile, InviteProfile::Disabled);
    }

    #[test]
    fn friends_and_favorites_are_supported_without_extra_user_config() {
        for profile in [InviteProfile::Friends, InviteProfile::Favorites] {
            let state = InviteState {
                enabled: true,
                profile,
                last_action: None,
            };
            let rules = invite_rules_from_state(&state).unwrap();
            assert!(rules.enabled);
            assert_eq!(rules.profile, profile);
        }
    }

    #[test]
    fn whitelist_and_custom_wait_for_real_persisted_configuration() {
        for profile in [InviteProfile::Whitelist, InviteProfile::Custom] {
            let state = InviteState {
                enabled: true,
                profile,
                last_action: None,
            };
            assert_eq!(invite_rules_from_state(&state), Err(profile));
        }
    }
}
