use super::*;
use nightshift_protocol::{InviteRuleSettings, SettingsState};

#[test]
fn converts_stored_session_to_vrchat_cookie_names() {
    let session = VrchatSession {
        auth_cookie: "auth-cookie".into(),
        two_factor_cookie: Some("2fa-cookie".into()),
        user_id: None,
    };
    assert_eq!(
        session_cookies(&session).cookie_header(),
        "auth=auth-cookie; twoFactorAuth=2fa-cookie"
    );
}

#[test]
fn disconnected_report_is_explicitly_offline() {
    assert_eq!(
        disconnected_report(),
        WorkerReport::VrchatRuntime {
            connected: false,
            display_name: None,
            status: VrchatStatus::Offline,
            last_invite_action: None,
            invite_error: None,
        }
    );
}

#[test]
fn pipeline_errors_extract_only_safe_message() {
    assert_eq!(
        pipeline_error_message(r#"{"err":"bad auth"}"#).as_deref(),
        Some("bad auth")
    );
    assert_eq!(
        pipeline_error_message(r#"{"err":{"message":"expired"}}"#).as_deref(),
        Some("expired")
    );
    assert_eq!(pipeline_error_message(r#"{"type":"notification"}"#), None);
}

#[test]
fn pipeline_server_error_redacts_raw_and_encoded_auth_token() {
    let message = "raw abc+/= and encoded abc%2B%2F%3D";
    assert_eq!(
        redact_pipeline_secret(message, "abc+/="),
        "raw <redacted> and encoded <redacted>"
    );
    assert_eq!(redact_pipeline_secret("ordinary error", ""), "ordinary error");
}

#[test]
fn invite_policy_change_includes_rule_edits_but_not_feedback_updates() {
    let current = InviteState {
        enabled: true,
        ..Default::default()
    };
    let mut rules_changed = current.clone();
    rules_changed.rules = InviteRuleSettings {
        whitelist: vec!["usr_friend".into()],
        ..Default::default()
    };
    assert!(invite_policy_changed(&current, &rules_changed));

    let mut feedback_only = current.clone();
    feedback_only.last_error = Some("test".into());
    assert!(!invite_policy_changed(&current, &feedback_only));
}

#[test]
fn invite_feedback_maps_success_and_failure_without_runtime_secrets() {
    let accepted = InviteHandlingResult {
        notification_id: "not_1".into(),
        sender_user_id: "usr_1".into(),
        sender_username: Some("Friend".into()),
        outcome: InviteHandlingOutcome::Execution(InviteExecutionOutcome::AcceptedRequest),
    };
    assert_eq!(
        invite_feedback(&accepted),
        (
            Some(InviteAction {
                kind: InviteActionKind::Accepted,
                display_name: Some("Friend".into()),
            }),
            None,
        )
    );

    let missing_instance = InviteHandlingResult {
        outcome: InviteHandlingOutcome::Execution(
            InviteExecutionOutcome::MissingCurrentInstance,
        ),
        ..accepted
    };
    let (action, error) = invite_feedback(&missing_instance);
    assert_eq!(action, None);
    assert!(error.unwrap().contains("instance"));
}

#[test]
fn runtime_feedback_uses_current_identity_and_status() {
    let runtime = Some(WorkerReport::VrchatRuntime {
        connected: true,
        display_name: Some("Hydra Employee".into()),
        status: VrchatStatus::AskMe,
        last_invite_action: None,
        invite_error: None,
    });
    assert_eq!(
        runtime_feedback_report(
            &runtime,
            Some(InviteAction {
                kind: InviteActionKind::Declined,
                display_name: Some("Friend".into()),
            }),
            None,
        ),
        Some(WorkerReport::VrchatRuntime {
            connected: true,
            display_name: Some("Hydra Employee".into()),
            status: VrchatStatus::AskMe,
            last_invite_action: Some(InviteAction {
                kind: InviteActionKind::Declined,
                display_name: Some("Friend".into()),
            }),
            invite_error: None,
        })
    );
}

#[test]
fn settings_map_to_status_policy() {
    let mut settings = SettingsState::default();
    settings.vrchat_status_automation_enabled = true;
    settings.sleep_vrchat_status = VrchatStatus::Busy;
    settings.wake_vrchat_status = VrchatStatus::JoinMe;
    assert_eq!(
        status_config(&settings),
        StatusAutomationConfig {
            enabled: true,
            sleep_status: VrchatStatus::Busy,
            wake_status: VrchatStatus::JoinMe,
        }
    );
}

#[tokio::test]
async fn disconnect_aborts_pending_status_task() {
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = ran.clone();
    let mut task = AbortTask(Some(tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        flag.store(true, std::sync::atomic::Ordering::Release);
    })));
    let mut generation = 7;

    cancel_pending_status_on_disconnect(false, &mut generation, &mut task);
    tokio::time::sleep(Duration::from_millis(80)).await;

    assert_eq!(generation, 8);
    assert!(!task.is_pending());
    assert!(!ran.load(std::sync::atomic::Ordering::Acquire));
}

#[tokio::test]
async fn session_change_aborts_pending_status_task() {
    let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = ran.clone();
    let mut task = AbortTask(Some(tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        flag.store(true, std::sync::atomic::Ordering::Release);
    })));
    let mut generation = 11;

    cancel_pending_status_on_session_change(true, &mut generation, &mut task);
    tokio::time::sleep(Duration::from_millis(80)).await;

    assert_eq!(generation, 12);
    assert!(!task.is_pending());
    assert!(!ran.load(std::sync::atomic::Ordering::Acquire));
}

#[test]
fn session_sync_reports_identity_changes() {
    let mut current = Some(SessionCookies {
        auth: "session-a".into(),
        two_factor_auth: None,
    });
    let same = current.clone();
    let replacement = Some(SessionCookies {
        auth: "session-b".into(),
        two_factor_auth: None,
    });
    let mut tasks = JoinSet::<Result<InviteHandlingResult>>::new();
    let mut recent = RecentInviteNotifications::default();

    assert!(!sync_invite_session(
        &mut current,
        same,
        &mut tasks,
        &mut recent,
    ));
    assert!(sync_invite_session(
        &mut current,
        replacement.clone(),
        &mut tasks,
        &mut recent,
    ));
    assert_eq!(current, replacement);
}

#[test]
fn status_writes_wait_at_least_for_debounce() {
    assert!(status_update_delay(None) >= STATUS_DEBOUNCE);
}

#[test]
fn recent_status_write_extends_delay_for_rate_limit() {
    let delay = status_update_delay(Some(Instant::now() - Duration::from_secs(5)));
    assert!(delay >= Duration::from_secs(4));
    assert!(delay <= Duration::from_secs(6));
}
