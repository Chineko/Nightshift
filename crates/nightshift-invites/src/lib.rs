use std::collections::HashSet;

use nightshift_protocol::{InviteActionKind, InviteProfile};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InviteContext {
    pub user_id: String,
    pub is_friend: bool,
    pub is_favorite: bool,
    pub is_whitelisted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InviteRules {
    pub enabled: bool,
    pub profile: InviteProfile,
    pub whitelist: HashSet<String>,
    pub custom_accept_friends: bool,
    pub custom_accept_favorites: bool,
    pub custom_accept_whitelist: bool,
    pub custom_fallback: InviteActionKind,
}

impl Default for InviteRules {
    fn default() -> Self {
        Self {
            enabled: false,
            profile: InviteProfile::Disabled,
            whitelist: HashSet::new(),
            custom_accept_friends: true,
            custom_accept_favorites: true,
            custom_accept_whitelist: true,
            custom_fallback: InviteActionKind::Ignored,
        }
    }
}

impl InviteRules {
    pub fn decide(&self, context: &InviteContext) -> InviteActionKind {
        if !self.enabled || self.profile == InviteProfile::Disabled {
            return InviteActionKind::Ignored;
        }

        let whitelisted = context.is_whitelisted || self.whitelist.contains(&context.user_id);

        match self.profile {
            InviteProfile::Disabled => InviteActionKind::Ignored,
            InviteProfile::Friends => accept_if(context.is_friend),
            InviteProfile::Favorites => accept_if(context.is_favorite),
            InviteProfile::Whitelist => accept_if(whitelisted),
            InviteProfile::Custom => {
                if (self.custom_accept_friends && context.is_friend)
                    || (self.custom_accept_favorites && context.is_favorite)
                    || (self.custom_accept_whitelist && whitelisted)
                {
                    InviteActionKind::Accepted
                } else {
                    safe_custom_fallback(self.custom_fallback)
                }
            }
        }
    }
}

fn safe_custom_fallback(value: InviteActionKind) -> InviteActionKind {
    match value {
        InviteActionKind::Declined => InviteActionKind::Declined,
        _ => InviteActionKind::Ignored,
    }
}

fn accept_if(matches: bool) -> InviteActionKind {
    if matches {
        InviteActionKind::Accepted
    } else {
        InviteActionKind::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> InviteContext {
        InviteContext {
            user_id: "usr_test".into(),
            ..Default::default()
        }
    }

    #[test]
    fn disabled_always_ignores() {
        let rules = InviteRules {
            enabled: false,
            profile: InviteProfile::Friends,
            ..Default::default()
        };
        let mut invite = context();
        invite.is_friend = true;

        assert_eq!(rules.decide(&invite), InviteActionKind::Ignored);
    }

    #[test]
    fn friends_profile_accepts_only_friends() {
        let rules = InviteRules {
            enabled: true,
            profile: InviteProfile::Friends,
            ..Default::default()
        };
        let mut invite = context();
        assert_eq!(rules.decide(&invite), InviteActionKind::Ignored);

        invite.is_friend = true;
        assert_eq!(rules.decide(&invite), InviteActionKind::Accepted);
    }

    #[test]
    fn favorites_profile_does_not_treat_all_friends_as_favorites() {
        let rules = InviteRules {
            enabled: true,
            profile: InviteProfile::Favorites,
            ..Default::default()
        };
        let mut invite = context();
        invite.is_friend = true;
        assert_eq!(rules.decide(&invite), InviteActionKind::Ignored);

        invite.is_favorite = true;
        assert_eq!(rules.decide(&invite), InviteActionKind::Accepted);
    }

    #[test]
    fn whitelist_accepts_explicit_ids() {
        let mut rules = InviteRules {
            enabled: true,
            profile: InviteProfile::Whitelist,
            ..Default::default()
        };
        rules.whitelist.insert("usr_test".into());

        assert_eq!(rules.decide(&context()), InviteActionKind::Accepted);
    }

    #[test]
    fn custom_profile_uses_fallback_when_no_rule_matches() {
        let rules = InviteRules {
            enabled: true,
            profile: InviteProfile::Custom,
            custom_accept_friends: false,
            custom_accept_favorites: false,
            custom_accept_whitelist: false,
            custom_fallback: InviteActionKind::Declined,
            ..Default::default()
        };

        assert_eq!(rules.decide(&context()), InviteActionKind::Declined);
    }

    #[test]
    fn custom_profile_accepts_matching_sources() {
        let rules = InviteRules {
            enabled: true,
            profile: InviteProfile::Custom,
            custom_accept_friends: true,
            custom_accept_favorites: false,
            custom_accept_whitelist: false,
            custom_fallback: InviteActionKind::Declined,
            ..Default::default()
        };
        let mut invite = context();
        invite.is_friend = true;

        assert_eq!(rules.decide(&invite), InviteActionKind::Accepted);
    }

    #[test]
    fn unsafe_custom_fallbacks_fail_closed() {
        for fallback in [InviteActionKind::Accepted, InviteActionKind::Asked] {
            let rules = InviteRules {
                enabled: true,
                profile: InviteProfile::Custom,
                custom_accept_friends: false,
                custom_accept_favorites: false,
                custom_accept_whitelist: false,
                custom_fallback: fallback,
                ..Default::default()
            };
            assert_eq!(rules.decide(&context()), InviteActionKind::Ignored);
        }
    }
}
