use nightshift_protocol::InviteActionKind;
use nightshift_vrchat_api::{NotificationKind, VrchatNotification};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InviteExecutionPlan {
    None,
    /// Accepting a requestInvite means inviting the requester to our known current instance.
    AcceptRequest {
        notification_id: String,
        sender_user_id: String,
        instance_id: String,
    },
    /// Declines are sent through VRChat's invite-response endpoint, then hidden after success.
    /// With no custom response message, responseSlot must be omitted rather than guessed.
    Decline {
        notification_id: String,
    },
    /// A request was policy-accepted but we cannot safely invite without a live instance.
    MissingCurrentInstance,
    /// Normal incoming invites need a separate join policy; never reinterpret Accepted as join.
    UnsupportedIncomingInviteAcceptance,
}

pub fn plan_invite_action(
    notification: &VrchatNotification,
    decision: InviteActionKind,
    current_instance: Option<&str>,
) -> InviteExecutionPlan {
    match decision {
        InviteActionKind::Ignored | InviteActionKind::Asked => InviteExecutionPlan::None,
        InviteActionKind::Declined => InviteExecutionPlan::Decline {
            notification_id: notification.id.clone(),
        },
        InviteActionKind::Accepted => match notification.kind {
            NotificationKind::RequestInvite => match current_instance {
                Some(instance_id) => InviteExecutionPlan::AcceptRequest {
                    notification_id: notification.id.clone(),
                    sender_user_id: notification.sender_user_id.clone(),
                    instance_id: instance_id.to_owned(),
                },
                None => InviteExecutionPlan::MissingCurrentInstance,
            },
            NotificationKind::Invite => InviteExecutionPlan::UnsupportedIncomingInviteAcceptance,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(kind: NotificationKind) -> VrchatNotification {
        VrchatNotification {
            id: "not_test".into(),
            kind,
            sender_user_id: "usr_test".into(),
            sender_username: Some("Test".into()),
        }
    }

    #[test]
    fn accepted_invite_request_requires_current_instance() {
        assert_eq!(
            plan_invite_action(
                &notification(NotificationKind::RequestInvite),
                InviteActionKind::Accepted,
                None,
            ),
            InviteExecutionPlan::MissingCurrentInstance
        );
        assert_eq!(
            plan_invite_action(
                &notification(NotificationKind::RequestInvite),
                InviteActionKind::Accepted,
                Some("wrld_test:instance"),
            ),
            InviteExecutionPlan::AcceptRequest {
                notification_id: "not_test".into(),
                sender_user_id: "usr_test".into(),
                instance_id: "wrld_test:instance".into(),
            }
        );
    }

    #[test]
    fn accepted_normal_invite_never_becomes_an_implicit_join() {
        assert_eq!(
            plan_invite_action(
                &notification(NotificationKind::Invite),
                InviteActionKind::Accepted,
                Some("wrld_test:instance"),
            ),
            InviteExecutionPlan::UnsupportedIncomingInviteAcceptance
        );
    }

    #[test]
    fn decline_works_for_both_notification_kinds_without_guessing_a_response_slot() {
        for kind in [NotificationKind::Invite, NotificationKind::RequestInvite] {
            assert_eq!(
                plan_invite_action(&notification(kind), InviteActionKind::Declined, None),
                InviteExecutionPlan::Decline {
                    notification_id: "not_test".into(),
                }
            );
        }
    }

    #[test]
    fn ignored_and_asked_are_non_actions() {
        for decision in [InviteActionKind::Ignored, InviteActionKind::Asked] {
            assert_eq!(
                plan_invite_action(&notification(NotificationKind::RequestInvite), decision, None),
                InviteExecutionPlan::None
            );
        }
    }
}
