use std::{collections::HashSet, sync::{Arc, Mutex}};

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
    /// Retained while protocol-v1 callers transition to persisted invite rule settings.
    ConfigurationUnavailable(InviteProfile),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InviteHandlingResult {
    pub notification_id: String,
    pub sender_user_id: String,
    pub sender_username: Option<String>,
    pub outcome: InviteHandlingOutcome,
}

pub fn invite_rules_from_state(state: &InviteState) -> InviteRules {
    InviteRules {
        enabled: state.enabled,
        profile: state.profile,
        whitelist: state.rules.whitelist.iter().cloned().collect::<HashSet<_>>(),
        custom_accept_friends: state.rules.accept_friends,
        custom_accept_favorites: state.rules.accept_favorites,
        custom_accept_whitelist: state.rules.accept_whitelist,
        custom_fallback: state.rules.fallback,
    }
}

fn relationship_context_needed(rules: &InviteRules) -> bool {
    match rules.profile {
        InviteProfile::Disabled | InviteProfile::Whitelist => false,
        InviteProfile::Friends | InviteProfile::Favorites => true,
        InviteProfile::Custom => rules.custom_accept_friends || rules.custom_accept_favorites,
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

    let rules = invite_rules_from_state(&invite_state);
    if !rules.enabled || rules.profile == InviteProfile::Disabled {
        return Ok(base(InviteHandlingOutcome::Execution(
            InviteExecutionOutcome::None,
        )));
    }

    let (is_friend, is_favorite) = if relationship_context_needed(&rules) {
        api.invite_sender_relationship(&cookies, &notification.sender_user_id)
            .await
            .context("resolving VRChat invite sender relationship")?
    } else {
        (false, false)
    };

    let decision = rules.decide(&InviteContext {
        user_id: notification.sender_user_id.clone(),
        is_friend,
        is_favorite,
        is_whitelisted: rules.whitelist.contains(&notification.sender_user_id),
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
    use nightshift_protocol::{InviteActionKind, InviteRuleSettings};

    #[test]
    fn disabled_policy_stays_disabled() {
        let rules = invite_rules_from_state(&InviteState::default());
        assert!(!rules.enabled);
        assert_eq!(rules.profile, InviteProfile::Disabled);
    }

    #[test]
    fn friends_and_favorites_are_supported_without_extra_user_config() {
        for profile in [InviteProfile::Friends, InviteProfile::Favorites] {
            let state = InviteState {
                enabled: true,
                profile,
                ..Default::default()
            };
            let rules = invite_rules_from_state(&state);
            assert!(rules.enabled);
            assert_eq!(rules.profile, profile);
        }
    }

    #[test]
    fn whitelist_uses_persisted_user_ids_without_relationship_lookup() {
        let state = InviteState {
            enabled: true,
            profile: InviteProfile::Whitelist,
            rules: InviteRuleSettings {
                whitelist: vec!["usr_allowed".into()],
                ..Default::default()
            },
            last_action: None,
        };
        let rules = invite_rules_from_state(&state);
        assert!(rules.whitelist.contains("usr_allowed"));
        assert!(!relationship_context_needed(&rules));
    }

    #[test]
    fn custom_settings_map_to_pure_rules() {
        let state = InviteState {
            enabled: true,
            profile: InviteProfile::Custom,
            rules: InviteRuleSettings {
                whitelist: vec!["usr_allowed".into()],
                accept_friends: false,
                accept_favorites: true,
                accept_whitelist: false,
                fallback: InviteActionKind::Declined,
            },
            last_action: None,
        };
        let rules = invite_rules_from_state(&state);
        assert!(!rules.custom_accept_friends);
        assert!(rules.custom_accept_favorites);
        assert!(!rules.custom_accept_whitelist);
        assert_eq!(rules.custom_fallback, InviteActionKind::Declined);
        assert!(relationship_context_needed(&rules));
    }

    #[test]
    fn custom_whitelist_only_does_not_require_social_lookup() {
        let state = InviteState {
            enabled: true,
            profile: InviteProfile::Custom,
            rules: InviteRuleSettings {
                accept_friends: false,
                accept_favorites: false,
                accept_whitelist: true,
                ..Default::default()
            },
            last_action: None,
        };
        assert!(!relationship_context_needed(&invite_rules_from_state(&state)));
    }
}
