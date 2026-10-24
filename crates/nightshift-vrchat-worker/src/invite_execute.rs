use anyhow::Result;
use nightshift_vrchat_api::{InviteRefused, SessionCookies, VrchatApiClient};

use crate::invite_plan::InviteExecutionPlan;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InviteExecutionOutcome {
    None,
    AcceptedRequest,
    Declined,
    /// VRChat refused inviting the requester to the current instance. The original notification
    /// remains visible so the request is not silently lost.
    InviteRefused { message: Option<String> },
    MissingCurrentInstance,
    UnsupportedIncomingInviteAcceptance,
}

pub async fn execute_invite_plan(
    api: &VrchatApiClient,
    cookies: &SessionCookies,
    plan: InviteExecutionPlan,
) -> Result<InviteExecutionOutcome> {
    match plan {
        InviteExecutionPlan::None => Ok(InviteExecutionOutcome::None),
        InviteExecutionPlan::MissingCurrentInstance => {
            Ok(InviteExecutionOutcome::MissingCurrentInstance)
        }
        InviteExecutionPlan::UnsupportedIncomingInviteAcceptance => {
            Ok(InviteExecutionOutcome::UnsupportedIncomingInviteAcceptance)
        }
        InviteExecutionPlan::AcceptRequest {
            notification_id,
            sender_user_id,
            instance_id,
        } => {
            if let Err(error) = api
                .invite_user(cookies, &sender_user_id, &instance_id, None)
                .await
            {
                if let Some(refused) = error.downcast_ref::<InviteRefused>() {
                    return Ok(InviteExecutionOutcome::InviteRefused {
                        message: refused.message.clone(),
                    });
                }
                return Err(error);
            }

            // Do not hide first: if VRChat refuses or the invite request fails, the user must
            // retain the original request in VRChat.
            api.hide_notification(cookies, &notification_id).await?;
            Ok(InviteExecutionOutcome::AcceptedRequest)
        }
        InviteExecutionPlan::Decline { notification_id } => {
            // No custom decline message exists yet, so there is no valid response-message slot to
            // send. Omitting responseSlot matches VRChat/Oyasumi behavior for a message-less reply.
            api.decline_invite_or_request(cookies, &notification_id, None)
                .await?;
            // Likewise, only remove the source notification after VRChat accepted the response.
            api.hide_notification(cookies, &notification_id).await?;
            Ok(InviteExecutionOutcome::Declined)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api() -> VrchatApiClient {
        VrchatApiClient::new().unwrap()
    }

    fn cookies() -> SessionCookies {
        SessionCookies {
            auth: "test".into(),
            two_factor_auth: None,
        }
    }

    #[tokio::test]
    async fn non_action_plans_never_touch_the_network() {
        let cases = [
            (
                InviteExecutionPlan::None,
                InviteExecutionOutcome::None,
            ),
            (
                InviteExecutionPlan::MissingCurrentInstance,
                InviteExecutionOutcome::MissingCurrentInstance,
            ),
            (
                InviteExecutionPlan::UnsupportedIncomingInviteAcceptance,
                InviteExecutionOutcome::UnsupportedIncomingInviteAcceptance,
            ),
        ];

        for (plan, expected) in cases {
            assert_eq!(execute_invite_plan(&api(), &cookies(), plan).await.unwrap(), expected);
        }
    }

    #[test]
    fn refused_invite_error_remains_typed_for_executor_classification() {
        let error: anyhow::Error = InviteRefused {
            message: Some("instance does not permit invites".into()),
            body: "{}".into(),
        }
        .into();
        let refused = error.downcast_ref::<InviteRefused>().unwrap();
        assert_eq!(
            refused.message.as_deref(),
            Some("instance does not permit invites")
        );
    }
}
