use super::*;

pub(crate) struct AbortTask(pub(crate) Option<JoinHandle<()>>);

impl AbortTask {
    pub(crate) fn replace(&mut self, task: JoinHandle<()>) {
        self.abort();
        self.0 = Some(task);
    }

    pub(crate) fn abort(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.0.is_some()
    }
}

impl Drop for AbortTask {
    fn drop(&mut self) {
        self.abort();
    }
}

pub(crate) struct RuntimeRefresh {
    pub(crate) report: WorkerReport,
    pub(crate) pipeline_auth_token: Option<String>,
    pub(crate) session_cookies: Option<SessionCookies>,
}

pub(crate) struct StatusTaskResult {
    pub(crate) generation: u64,
    pub(crate) result: Result<RuntimeRefresh>,
}

pub(crate) fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

pub(crate) fn disconnected_report() -> WorkerReport {
    WorkerReport::VrchatRuntime {
        connected: false,
        display_name: None,
        status: VrchatStatus::Offline,
        last_invite_action: None,
        invite_error: None,
    }
}

pub(crate) fn disconnected_refresh() -> RuntimeRefresh {
    RuntimeRefresh {
        report: disconnected_report(),
        pipeline_auth_token: None,
        session_cookies: None,
    }
}

pub(crate) fn session_cookies(session: &VrchatSession) -> SessionCookies {
    SessionCookies {
        auth: session.auth_cookie.clone(),
        two_factor_auth: session.two_factor_cookie.clone(),
    }
}

pub(crate) fn status_config(settings: &nightshift_protocol::SettingsState) -> StatusAutomationConfig {
    StatusAutomationConfig {
        enabled: settings.vrchat_status_automation_enabled,
        sleep_status: settings.sleep_vrchat_status,
        wake_status: settings.wake_vrchat_status,
    }
}

pub(crate) fn status_for_mode(
    automation: StatusAutomationConfig,
    sleeping: bool,
) -> VrchatStatus {
    if sleeping {
        automation.sleep_status
    } else {
        automation.wake_status
    }
}

pub(crate) fn report_runtime(report: &WorkerReport) -> (bool, VrchatStatus) {
    match report {
        WorkerReport::VrchatRuntime {
            connected, status, ..
        } => (*connected, *status),
        _ => (false, VrchatStatus::Offline),
    }
}

pub(crate) fn status_update_delay(last_write: Option<Instant>) -> Duration {
    let rate_limit_wait = last_write
        .map(|last| STATUS_MIN_INTERVAL.saturating_sub(last.elapsed()))
        .unwrap_or_default();
    STATUS_DEBOUNCE.max(rate_limit_wait)
}

pub(crate) fn cancel_pending_status_on_disconnect(
    connected: bool,
    status_generation: &mut u64,
    status_task: &mut AbortTask,
) {
    if !connected && status_task.is_pending() {
        *status_generation = status_generation.wrapping_add(1);
        status_task.abort();
    }
}

pub(crate) fn cancel_pending_status_on_session_change(
    changed: bool,
    status_generation: &mut u64,
    status_task: &mut AbortTask,
) {
    if changed && status_task.is_pending() {
        // A sleep/wake edge belongs to the account that observed it. Do not mail it to the next tenant.
        *status_generation = status_generation.wrapping_add(1);
        status_task.abort();
    }
}

pub(crate) fn sync_invite_session(
    current: &mut Option<SessionCookies>,
    next: Option<SessionCookies>,
    tasks: &mut JoinSet<Result<InviteHandlingResult>>,
    recent: &mut RecentInviteNotifications,
) -> bool {
    if current.as_ref() == next.as_ref() {
        return false;
    }
    tasks.abort_all();
    recent.clear();
    *current = next;
    true
}

pub(crate) fn invite_policy_changed(current: &InviteState, next: &InviteState) -> bool {
    current.enabled != next.enabled
        || current.profile != next.profile
        || current.rules != next.rules
}

pub(crate) fn sync_invite_state(
    current: &mut InviteState,
    next: InviteState,
    tasks: &mut JoinSet<Result<InviteHandlingResult>>,
) {
    if invite_policy_changed(current, &next) {
        tasks.abort_all();
    }
    *current = next;
}

pub(crate) fn log_invite_result(result: &InviteHandlingResult) {
    let sender_username = result.sender_username.as_deref().unwrap_or("<unknown>");

    let InviteHandlingOutcome::Execution(outcome) = &result.outcome;
    match outcome {
        InviteExecutionOutcome::None => {}
        InviteExecutionOutcome::AcceptedRequest => {
            info!(
                notification_id = %result.notification_id,
                sender_user_id = %result.sender_user_id,
                sender_username = %sender_username,
                "accepted VRChat invite request"
            );
        }
        InviteExecutionOutcome::Declined => {
            info!(
                notification_id = %result.notification_id,
                sender_user_id = %result.sender_user_id,
                sender_username = %sender_username,
                "declined VRChat invite notification"
            );
        }
        InviteExecutionOutcome::InviteRefused { message } => {
            warn!(
                notification_id = %result.notification_id,
                sender_user_id = %result.sender_user_id,
                sender_username = %sender_username,
                vrchat_message = message.as_deref().unwrap_or("<none>"),
                "VRChat refused invite-request acceptance; original request was left visible"
            );
        }
        InviteExecutionOutcome::MissingCurrentInstance => {
            warn!(
                notification_id = %result.notification_id,
                sender_user_id = %result.sender_user_id,
                sender_username = %sender_username,
                "invite request matched policy but current VRChat instance is unavailable"
            );
        }
        InviteExecutionOutcome::UnsupportedIncomingInviteAcceptance => {
            info!(
                notification_id = %result.notification_id,
                sender_user_id = %result.sender_user_id,
                sender_username = %sender_username,
                "incoming VRChat invite matched accept policy, but automatic joining is not implemented"
            );
        }
    }
}

pub(crate) fn invite_feedback(result: &InviteHandlingResult) -> (Option<InviteAction>, Option<String>) {
    let action = |kind| InviteAction {
        kind,
        display_name: result.sender_username.clone(),
    };

    match &result.outcome {
        InviteHandlingOutcome::Execution(InviteExecutionOutcome::None) => (None, None),
        InviteHandlingOutcome::Execution(InviteExecutionOutcome::AcceptedRequest) => {
            (Some(action(InviteActionKind::Accepted)), None)
        }
        InviteHandlingOutcome::Execution(InviteExecutionOutcome::Declined) => {
            (Some(action(InviteActionKind::Declined)), None)
        }
        InviteHandlingOutcome::Execution(InviteExecutionOutcome::InviteRefused { message }) => (
            None,
            Some(match message {
                Some(message) => format!("VRChat refused the invite: {message}"),
                None => "VRChat refused the invite to the current instance".into(),
            }),
        ),
        InviteHandlingOutcome::Execution(InviteExecutionOutcome::MissingCurrentInstance) => (
            None,
            Some("Current VRChat instance is unavailable; invite request was left visible".into()),
        ),
        InviteHandlingOutcome::Execution(
            InviteExecutionOutcome::UnsupportedIncomingInviteAcceptance,
        ) => (
            None,
            Some("Automatic joining for incoming VRChat invites is not implemented".into()),
        ),
    }
}

pub(crate) fn runtime_feedback_report(
    runtime: &Option<WorkerReport>,
    last_invite_action: Option<InviteAction>,
    invite_error: Option<String>,
) -> Option<WorkerReport> {
    let WorkerReport::VrchatRuntime {
        connected,
        display_name,
        status,
        ..
    } = runtime.as_ref()?
    else {
        return None;
    };
    if !connected {
        return None;
    }

    Some(WorkerReport::VrchatRuntime {
        connected: true,
        display_name: display_name.clone(),
        status: *status,
        last_invite_action,
        invite_error,
    })
}

pub(crate) async fn load_session(store: VrchatSessionStore) -> Result<Option<VrchatSession>> {
    tokio::task::spawn_blocking(move || store.load())
        .await
        .context("VRChat session keyring task stopped")?
}

pub(crate) async fn save_session(store: VrchatSessionStore, session: VrchatSession) -> Result<()> {
    tokio::task::spawn_blocking(move || store.save(&session))
        .await
        .context("VRChat session keyring task stopped")?
}

pub(crate) async fn validated_user(
    api: &VrchatApiClient,
    store: &VrchatSessionStore,
) -> Result<Option<(VrchatSession, nightshift_vrchat_api::CurrentUserSummary)>> {
    let mut session = match load_session(store.clone()).await? {
        Some(session) => session,
        None => return Ok(None),
    };
    let cookies = session_cookies(&session);
    let user = {
        let _permit = shared_vrchat_requests()
            .acquire(RequestClass::SessionRead)
            .await;
        api.current_user(&cookies).await?
    };

    if let Some(expected_user_id) = session.user_id.as_deref() {
        if expected_user_id != user.id {
            bail!(
                "VRChat keyring session account mismatch: expected {expected_user_id}, got {}",
                user.id
            );
        }
    } else {
        session.user_id = Some(user.id.clone());
        if let Err(error) = save_session(store.clone(), session.clone()).await {
            warn!(%error, "failed to attach VRChat user id to stored session");
        }
    }

    Ok(Some((session, user)))
}

pub(crate) async fn refresh_runtime(
    api: &VrchatApiClient,
    store: &VrchatSessionStore,
) -> RuntimeRefresh {
    let (session, user) = match validated_user(api, store).await {
        Ok(Some(value)) => value,
        Ok(None) => return disconnected_refresh(),
        Err(error) => {
            warn!(%error, "VRChat session validation failed");
            return disconnected_refresh();
        }
    };
    let cookies = session_cookies(&session);

    RuntimeRefresh {
        report: WorkerReport::VrchatRuntime {
            connected: true,
            display_name: Some(user.display_name),
            status: user.status,
            last_invite_action: None,
            invite_error: None,
        },
        pipeline_auth_token: Some(cookies.auth.clone()),
        session_cookies: Some(cookies),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_policy_selects_configured_sleep_and_wake_targets() {
        let automation = StatusAutomationConfig {
            enabled: true,
            sleep_status: VrchatStatus::AskMe,
            wake_status: VrchatStatus::Active,
        };
        assert_eq!(status_for_mode(automation, true), VrchatStatus::AskMe);
        assert_eq!(status_for_mode(automation, false), VrchatStatus::Active);
    }
}
