use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow};
use nightshift_invites::{InviteContext, InviteRules};
use nightshift_protocol::{InviteProfile, InviteState};
use nightshift_vrchat_api::{NotificationKind, SessionCookies, VrchatApiClient, VrchatNotification};

use crate::{
    instance_context::InstanceSource,
    invite_execute::{InviteExecutionOutcome, execute_invite_plan},
    invite_plan::plan_invite_action,
    request_coordinator::{RequestClass, shared_vrchat_requests},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InviteHandlingOutcome {
    Execution(InviteExecutionOutcome),
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
        blacklist: state.rules.blacklist.iter().cloned().collect::<HashSet<_>>(),
        custom_accept_friends: state.rules.accept_friends,
        custom_accept_favorites: state.rules.accept_favorites,
        custom_accept_whitelist: state.rules.accept_whitelist,
        custom_fallback: state.rules.fallback,
    }
}

fn relationship_needs(rules: &InviteRules) -> (bool, bool) {
    match rules.profile {
        InviteProfile::Disabled | InviteProfile::Whitelist => (false, false),
        InviteProfile::Friends => (true, false),
        InviteProfile::Favorites => (false, true),
        InviteProfile::Custom => (
            rules.custom_accept_friends,
            rules.custom_accept_favorites,
        ),
    }
}

async fn resolve_relationship_context(
    api: &VrchatApiClient,
    cookies: &SessionCookies,
    user_id: &str,
    rules: &InviteRules,
) -> Result<(bool, bool)> {
    let (need_friend, need_favorite) = relationship_needs(rules);
    let is_favorite = if need_favorite {
        {
            let _permit = shared_vrchat_requests()
                .acquire(RequestClass::RelationshipRead)
                .await;
            api.invite_sender_is_favorite(cookies, user_id).await
        }
        .context("checking whether VRChat invite sender is a favorite friend")?
    } else {
        false
    };

    // Favorite friends are necessarily friends, so a positive favorite lookup also satisfies the
    // friend fact without a second HTTP request when Custom enables both sources.
    let is_friend = if is_favorite {
        true
    } else if need_friend {
        {
            let _permit = shared_vrchat_requests()
                .acquire(RequestClass::RelationshipRead)
                .await;
            api.invite_sender_is_friend(cookies, user_id).await
        }
        .context("checking whether VRChat invite sender is a friend")?
    } else {
        false
    };

    Ok((is_friend, is_favorite))
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

    let (is_friend, is_favorite) = resolve_relationship_context(
        &api,
        &cookies,
        &notification.sender_user_id,
        &rules,
    )
    .await?;

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
        assert_eq!(relationship_needs(&rules), (false, false));
    }

    #[test]
    fn simple_profiles_request_only_the_fact_they_use() {
        let friends = invite_rules_from_state(&InviteState {
            enabled: true,
            profile: InviteProfile::Friends,
            ..Default::default()
        });
        let favorites = invite_rules_from_state(&InviteState {
            enabled: true,
            profile: InviteProfile::Favorites,
            ..Default::default()
        });

        assert_eq!(relationship_needs(&friends), (true, false));
        assert_eq!(relationship_needs(&favorites), (false, true));
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
            last_error: None,
        };
        let rules = invite_rules_from_state(&state);
        assert!(rules.whitelist.contains("usr_allowed"));
        assert_eq!(relationship_needs(&rules), (false, false));
    }

    #[test]
    fn blacklist_maps_to_pure_rules() {
        let state = InviteState {
            enabled: true,
            profile: InviteProfile::Friends,
            rules: InviteRuleSettings {
                blacklist: vec!["usr_blocked".into()],
                ..Default::default()
            },
            last_action: None,
            last_error: None,
        };
        let rules = invite_rules_from_state(&state);
        assert!(rules.blacklist.contains("usr_blocked"));
    }

    #[test]
    fn custom_settings_map_to_pure_rules_and_lookup_needs() {
        let state = InviteState {
            enabled: true,
            profile: InviteProfile::Custom,
            rules: InviteRuleSettings {
                whitelist: vec!["usr_allowed".into()],
                blacklist: vec![],
                accept_friends: false,
                accept_favorites: true,
                accept_whitelist: false,
                fallback: InviteActionKind::Declined,
            },
            last_action: None,
            last_error: None,
        };
        let rules = invite_rules_from_state(&state);
        assert!(!rules.custom_accept_friends);
        assert!(rules.custom_accept_favorites);
        assert!(!rules.custom_accept_whitelist);
        assert_eq!(rules.custom_fallback, InviteActionKind::Declined);
        assert_eq!(relationship_needs(&rules), (false, true));
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
            last_error: None,
        };
        assert_eq!(
            relationship_needs(&invite_rules_from_state(&state)),
            (false, false)
        );
    }
}
