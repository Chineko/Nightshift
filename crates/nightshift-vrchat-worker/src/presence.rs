use super::*;
use crate::runtime::{
    AbortTask, RuntimeRefresh, StatusTaskResult, session_cookies, status_update_delay, validated_user,
};

pub(crate) fn cancel_status_edge(
    edge: bool,
    generation: &mut u64,
    task: &mut AbortTask,
) {
    if edge {
        *generation = generation.wrapping_add(1);
        task.abort();
    }
}

pub(crate) fn schedule_status_update(
    task: &mut AbortTask,
    result_tx: mpsc::UnboundedSender<StatusTaskResult>,
    api: VrchatApiClient,
    store: VrchatSessionStore,
    target_status: VrchatStatus,
    expected_session: Option<SessionCookies>,
    last_write: Option<Instant>,
    generation: u64,
) {
    let delay = status_update_delay(last_write);
    info!(
        generation,
        target_status = ?target_status,
        delay_ms = delay.as_millis(),
        "scheduling VRChat status update"
    );
    task.replace(tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let result = set_runtime_status(
            api,
            store,
            target_status,
            expected_session,
            generation,
        )
        .await;
        let _ = result_tx.send(StatusTaskResult { generation, result });
    }));
}

async fn set_runtime_status(
    api: VrchatApiClient,
    store: VrchatSessionStore,
    target_status: VrchatStatus,
    expected_session: Option<SessionCookies>,
    generation: u64,
) -> Result<RuntimeRefresh> {
    let Some((session, user)) = validated_user(&api, &store).await? else {
        bail!("no VRChat session is stored");
    };
    let cookies = session_cookies(&session);
    if expected_session.as_ref() != Some(&cookies) {
        bail!("VRChat session changed before pending status update");
    }

    info!(
        generation,
        current_status = ?user.status,
        target_status = ?target_status,
        "sending VRChat status update"
    );

    {
        let _permit = shared_vrchat_requests()
            .acquire(RequestClass::StatusWrite)
            .await;
        // Deliberately send the configured status even when the last observed status already
        // matches. Sleep/wake edges are commands, not cache reconciliation.
        api.set_status(&cookies, &user.id, target_status).await?;
    }

    info!(
        generation,
        requested_status = ?target_status,
        "VRChat status update accepted"
    );

    Ok(RuntimeRefresh {
        report: WorkerReport::VrchatRuntime {
            connected: true,
            display_name: Some(user.display_name),
            status: target_status,
            last_invite_action: None,
            invite_error: None,
        },
        pipeline_auth_token: Some(cookies.auth.clone()),
        session_cookies: Some(cookies),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_sleep_wake_edge_invalidates_pending_work() {
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = ran.clone();
        let mut task = AbortTask(Some(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, std::sync::atomic::Ordering::Release);
        })));
        let mut generation = 12;

        cancel_status_edge(true, &mut generation, &mut task);
        tokio::time::sleep(Duration::from_millis(80)).await;

        assert_eq!(generation, 13);
        assert!(!task.is_pending());
        assert!(!ran.load(std::sync::atomic::Ordering::Acquire));
    }
}
