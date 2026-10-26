use super::*;
use crate::runtime::{
    AbortTask, RuntimeRefresh, StatusTaskResult, session_cookies, status_update_delay, user_presence,
    validated_user,
};
use nightshift_vrchat_status::VrchatPresence;

#[derive(Clone, Default)]
pub(crate) struct PresenceCaptureSlot(Arc<Mutex<Option<(u64, VrchatPresence)>>>);

impl PresenceCaptureSlot {
    pub(crate) fn store_once(&self, generation: u64, presence: VrchatPresence) {
        let Ok(mut slot) = self.0.lock() else {
            return;
        };
        if slot.is_none() {
            *slot = Some((generation, presence));
        }
    }

    pub(crate) fn take(&self, generation: u64) -> Option<VrchatPresence> {
        let Ok(mut slot) = self.0.lock() else {
            return None;
        };
        match slot.as_ref() {
            Some((stored_generation, _)) if *stored_generation == generation => {
                slot.take().map(|(_, presence)| presence)
            }
            _ => None,
        }
    }

    pub(crate) fn clear(&self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }
}

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

pub(crate) fn sleep_target(current: &VrchatPresence, status: VrchatStatus) -> VrchatPresence {
    // Coarse automation gets to change the coarse status. User-authored text stays user-authored.
    VrchatPresence::new(status, current.status_description.clone())
}

pub(crate) fn schedule_sleep_status_update(
    task: &mut AbortTask,
    result_tx: mpsc::UnboundedSender<StatusTaskResult>,
    api: VrchatApiClient,
    store: VrchatSessionStore,
    sleep_status: VrchatStatus,
    expected_session: Option<SessionCookies>,
    last_write: Option<Instant>,
    generation: u64,
    capture_slot: PresenceCaptureSlot,
    capture_original: bool,
) {
    let delay = status_update_delay(last_write);
    task.replace(tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let result = set_runtime_sleep_presence(
            api,
            store,
            sleep_status,
            expected_session,
            capture_slot,
            generation,
            capture_original,
        )
        .await;
        let _ = result_tx.send(StatusTaskResult { generation, result });
    }));
}

async fn set_runtime_sleep_presence(
    api: VrchatApiClient,
    store: VrchatSessionStore,
    sleep_status: VrchatStatus,
    expected_session: Option<SessionCookies>,
    capture_slot: PresenceCaptureSlot,
    generation: u64,
    capture_original: bool,
) -> Result<RuntimeRefresh> {
    let Some((session, user)) = validated_user(&api, &store).await? else {
        bail!("no VRChat session is stored");
    };
    let cookies = session_cookies(&session);
    if expected_session.as_ref() != Some(&cookies) {
        bail!("VRChat session changed before pending presence update");
    }

    let current = user_presence(&user);
    if capture_original {
        capture_slot.store_once(generation, current.clone());
    }

    // Cancellation point after capture, before mutation. A quick wake can now grab the return
    // ticket and abort this task without the sleep write escaping from the basement.
    tokio::task::yield_now().await;

    let target = sleep_target(&current, sleep_status);
    let verified = if current != target {
        let _permit = shared_vrchat_requests()
            .acquire(RequestClass::StatusWrite)
            .await;
        api.set_presence(
            &cookies,
            &user.id,
            target.status,
            &target.status_description,
        )
        .await?
    } else {
        user
    };
    let presence = user_presence(&verified);

    Ok(RuntimeRefresh {
        report: WorkerReport::VrchatRuntime {
            connected: true,
            display_name: Some(verified.display_name),
            status: presence.status,
            last_invite_action: None,
            invite_error: None,
        },
        presence: Some(presence),
        pipeline_auth_token: Some(cookies.auth.clone()),
        session_cookies: Some(cookies),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_target_preserves_user_status_text() {
        let current = VrchatPresence::new(VrchatStatus::Active, "Building pillow fort");
        assert_eq!(
            sleep_target(&current, VrchatStatus::AskMe),
            VrchatPresence::new(VrchatStatus::AskMe, "Building pillow fort")
        );
    }

    #[test]
    fn capture_slot_is_generation_scoped_and_one_shot() {
        let slot = PresenceCaptureSlot::default();
        let captured = VrchatPresence::new(VrchatStatus::Busy, "Do not disturb");
        slot.store_once(7, captured.clone());
        slot.store_once(8, VrchatPresence::new(VrchatStatus::Active, "wrong"));
        assert_eq!(slot.take(8), None);
        assert_eq!(slot.take(7), Some(captured));
        assert_eq!(slot.take(7), None);
    }

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
