use super::*;

#[tokio::test]
async fn ipc_line_reader_preserves_multiple_frames() {
    let (mut writer, mut reader) = tokio::io::duplex(128);
    let write_task = tokio::spawn(async move {
        writer.write_all(b"first\nsecond\r\n").await.unwrap();
    });
    let mut buffer = Vec::new();

    assert_eq!(
        read_ipc_line(&mut reader, &mut buffer).await.unwrap().as_deref(),
        Some("first")
    );
    assert_eq!(
        read_ipc_line(&mut reader, &mut buffer).await.unwrap().as_deref(),
        Some("second")
    );
    write_task.await.unwrap();
}

#[tokio::test]
async fn ipc_line_reader_rejects_oversized_frames() {
    let (mut writer, mut reader) = tokio::io::duplex(MAX_IPC_LINE_BYTES + 2);
    let write_task = tokio::spawn(async move {
        writer
            .write_all(&vec![b'x'; MAX_IPC_LINE_BYTES + 1])
            .await
            .unwrap();
    });
    let mut buffer = Vec::new();

    let error = read_ipc_line(&mut reader, &mut buffer).await.unwrap_err();
    assert!(error.to_string().contains("exceeds"));
    write_task.await.unwrap();
}

#[tokio::test]
async fn repeated_same_transition_is_idempotent() {
    let store = StateStore::new(AppState::default());
    store.apply(UiCommand::EnterSleep).await;
    let first = store.transition_generation.load(Ordering::Acquire);

    store.apply(UiCommand::EnterSleep).await;
    let second = store.transition_generation.load(Ordering::Acquire);

    assert_eq!(first, second);
    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.sleep.phase, SleepPhase::EnteringSleep);
}

#[tokio::test]
async fn stale_timer_cannot_finish_reentered_phase() {
    let store = StateStore::new(AppState::default());
    store.apply(UiCommand::EnterSleep).await;
    let stale = store.transition_generation.load(Ordering::Acquire);

    store.apply(UiCommand::Wake).await;
    store.apply(UiCommand::EnterSleep).await;
    let current = store.transition_generation.load(Ordering::Acquire);
    assert_ne!(stale, current);

    store
        .complete_transition(stale, SleepPhase::EnteringSleep, SleepPhase::Sleeping, 0.06)
        .await;

    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.sleep.phase, SleepPhase::EnteringSleep);
    assert!(snapshot.brightness.transitioning);
}

#[tokio::test]
async fn overlay_settings_are_clamped_and_applied() {
    let store = StateStore::new(AppState::default());

    store.apply(UiCommand::SetOverlaySpawnDistance(99.0)).await;
    store.apply(UiCommand::SetOverlayLaserEnabled(false)).await;
    store
        .apply(UiCommand::SetOverlaySpawnMode(
            OverlaySpawnMode::RestoreLastPosition,
        ))
        .await;

    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.overlay.spawn_distance_m, 5.0);
    assert!(!snapshot.overlay.laser_enabled);
    assert_eq!(
        snapshot.overlay.spawn_mode,
        OverlaySpawnMode::RestoreLastPosition
    );

    store.apply(UiCommand::SetOverlaySpawnDistance(0.0)).await;
    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.overlay.spawn_distance_m, 0.25);
}

#[tokio::test]
async fn overlay_runtime_pose_reports_are_scoped_normalized_and_stored() {
    let store = StateStore::new(AppState::default());
    let report = WorkerReport::OverlayRuntimePose {
        pose: OverlayPose {
            position: [1.0, 2.0, 3.0],
            orientation_xyzw: [0.0, 0.0, 0.0, 2.0],
        },
    };

    assert!(
        !store
            .apply_worker_report("nightshift-osc", report.clone())
            .await
    );
    assert!(
        store
            .apply_worker_report("nightshift-overlay", report)
            .await
    );

    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(
        snapshot.overlay.remembered_pose,
        Some(OverlayPose {
            position: [1.0, 2.0, 3.0],
            orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
        })
    );
}

#[tokio::test]
async fn invalid_overlay_runtime_pose_report_is_ignored() {
    let store = StateStore::new(AppState::default());
    assert!(
        store
            .apply_worker_report(
                "nightshift-overlay",
                WorkerReport::OverlayRuntimePose {
                    pose: OverlayPose {
                        position: [f32::NAN, 0.0, 0.0],
                        orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
                    },
                },
            )
            .await
    );
    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.overlay.remembered_pose, None);
}

#[tokio::test]
async fn hr_settings_are_clamped_and_reconfigure_detector() {
    let store = StateStore::new(AppState::default());

    store.apply(UiCommand::SetHrDeviceFilter(Some("  Polar H10  ".into()))).await;
    store.apply(UiCommand::SetHrThreshold(999)).await;
    store.apply(UiCommand::SetHrCalmPeriod(1)).await;
    store.apply(UiCommand::SetHrCooldown(u64::MAX)).await;

    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.heart_rate.device_filter.as_deref(), Some("Polar H10"));
    assert_eq!(snapshot.heart_rate.threshold_bpm, 240);
    assert_eq!(snapshot.heart_rate.calm_period_seconds, 10);
    assert_eq!(snapshot.heart_rate.cooldown_seconds, 24 * 60 * 60);

    let config = store.hr_detector.lock().await.config();
    assert_eq!(config.heart_rate_threshold_bpm, 240);
    assert_eq!(config.calm_period_ms, 10_000);
    assert_eq!(config.manual_wake_cooldown_ms, 24 * 60 * 60 * 1_000);

    store.apply(UiCommand::SetHrDeviceFilter(Some("   ".into()))).await;
    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.heart_rate.device_filter, None);
}

#[tokio::test]
async fn persisted_hr_tuning_initializes_detector() {
    let mut state = AppState::default();
    state.heart_rate.threshold_bpm = 72;
    state.heart_rate.calm_period_seconds = 45;
    state.heart_rate.cooldown_seconds = 300;
    let store = StateStore::new(state);

    let config = store.hr_detector.lock().await.config();
    assert_eq!(config.heart_rate_threshold_bpm, 72);
    assert_eq!(config.calm_period_ms, 45_000);
    assert_eq!(config.manual_wake_cooldown_ms, 300_000);
}

#[test]
fn hr_device_snapshots_are_bounded_normalized_and_deduplicated() {
    let mut devices = vec![
        HrDeviceInfo {
            id: " same ".into(),
            address: " AA ".into(),
            name: Some(" Polar ".into()),
        },
        HrDeviceInfo {
            id: "same".into(),
            address: "BB".into(),
            name: None,
        },
    ];
    devices.extend((0..40).map(|n| HrDeviceInfo {
        id: format!("id-{n}"),
        address: format!("addr-{n}"),
        name: None,
    }));

    let devices = normalize_hr_devices(devices);
    assert_eq!(devices.len(), MAX_HR_DISCOVERED_DEVICES);
    assert_eq!(devices.iter().filter(|device| device.id == "same").count(), 1);
    assert_eq!(devices[0].id, "same");
    assert_eq!(devices[0].address, "AA");
    assert_eq!(devices[0].name.as_deref(), Some("Polar"));
}

#[tokio::test]
async fn hr_provider_reports_are_worker_scoped_and_manage_health_state() {
    let store = StateStore::new(AppState::default());
    assert!(
        store
            .apply_worker_report(
                "nightshift-hr",
                WorkerReport::HeartRateDevices {
                    devices: vec![HrDeviceInfo {
                        id: "polar".into(),
                        address: "AA:BB".into(),
                        name: Some("Polar H10".into()),
                    }],
                },
            )
            .await
    );
    assert!(
        !store
            .apply_worker_report(
                "nightshift-osc",
                WorkerReport::HeartRateDevices { devices: vec![] },
            )
            .await
    );
    assert!(
        store
            .apply_worker_report(
                "nightshift-hr",
                WorkerReport::HeartRateProviderRuntime {
                    status: HrProviderStatus::Error,
                    connected_device: None,
                    last_error: Some("preferred device not found".into()),
                },
            )
            .await
    );

    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.heart_rate.provider_status, HrProviderStatus::Error);
    assert_eq!(snapshot.heart_rate.last_error.as_deref(), Some("preferred device not found"));
    assert_eq!(snapshot.heart_rate.available_devices.len(), 1);

    store
        .apply_worker_report(
            "nightshift-hr",
            WorkerReport::HeartRateProviderRuntime {
                status: HrProviderStatus::Searching,
                connected_device: None,
                last_error: None,
            },
        )
        .await;
    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.heart_rate.last_error.as_deref(), Some("preferred device not found"));
    assert!(snapshot.heart_rate.available_devices.is_empty());

    store
        .apply_worker_report(
            "nightshift-hr",
            WorkerReport::HeartRateProviderRuntime {
                status: HrProviderStatus::Connected,
                connected_device: Some("Polar H10".into()),
                last_error: None,
            },
        )
        .await;
    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.heart_rate.provider_status, HrProviderStatus::Connected);
    assert_eq!(snapshot.heart_rate.connected_device.as_deref(), Some("Polar H10"));
    assert_eq!(snapshot.heart_rate.last_error, None);
}

#[tokio::test]
async fn osc_worker_report_updates_runtime_state() {
    let store = StateStore::new(AppState::default());
    assert!(
        store
            .apply_worker_report(
                "nightshift-osc",
                WorkerReport::OscRuntime {
                    connected: true,
                    avatar_id: Some("avtr_test".into()),
                    avatar_parameter_count: 42,
                    gogo_detected: true,
                },
            )
            .await
    );

    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert!(snapshot.osc.connected);
    assert_eq!(snapshot.osc.avatar_id.as_deref(), Some("avtr_test"));
    assert_eq!(snapshot.osc.avatar_parameter_count, 42);
    assert!(snapshot.osc.gogo_detected);
}

#[tokio::test]
async fn overlay_can_report_pose_but_osc_worker_cannot() {
    let store = StateStore::new(AppState::default());
    assert!(
        store
            .apply_worker_report(
                "nightshift-overlay",
                WorkerReport::SleepingPose {
                    pose: SleepingPose::Left,
                },
            )
            .await
    );
    assert!(
        !store
            .apply_worker_report(
                "nightshift-osc",
                WorkerReport::SleepingPose {
                    pose: SleepingPose::Right,
                },
            )
            .await
    );

    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.sleep.pose, SleepingPose::Left);
}

#[tokio::test]
async fn heart_rate_reports_are_worker_scoped() {
    let store = StateStore::new(AppState::default());
    assert!(
        store
            .apply_worker_report(
                "nightshift-hr",
                WorkerReport::HeartRateRuntime {
                    connected: true,
                    bpm: Some(55),
                },
            )
            .await
    );
    assert!(
        !store
            .apply_worker_report(
                "nightshift-osc",
                WorkerReport::HeartRateRuntime {
                    connected: true,
                    bpm: Some(90),
                },
            )
            .await
    );

    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert!(snapshot.heart_rate.connected);
    assert_eq!(snapshot.heart_rate.bpm, Some(55));
}

#[tokio::test]
async fn vrchat_reports_are_worker_scoped_and_invite_feedback_is_additive() {
    let store = StateStore::new(AppState::default());
    let accepted = InviteAction {
        kind: InviteActionKind::Accepted,
        display_name: Some("Friend".into()),
    };
    assert!(
        store
            .apply_worker_report(
                "nightshift-vrchat",
                WorkerReport::VrchatRuntime {
                    connected: true,
                    display_name: Some("Hydra Employee".into()),
                    status: VrchatStatus::AskMe,
                    last_invite_action: Some(accepted.clone()),
                    invite_error: None,
                },
            )
            .await
    );
    assert!(
        !store
            .apply_worker_report(
                "nightshift-osc",
                WorkerReport::VrchatRuntime {
                    connected: true,
                    display_name: Some("Impostor".into()),
                    status: VrchatStatus::Busy,
                    last_invite_action: None,
                    invite_error: Some("should not apply".into()),
                },
            )
            .await
    );

    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert!(snapshot.vrchat.connected);
    assert_eq!(snapshot.vrchat.display_name.as_deref(), Some("Hydra Employee"));
    assert_eq!(snapshot.vrchat.status, VrchatStatus::AskMe);
    assert_eq!(snapshot.invites.last_action, Some(accepted));
    assert_eq!(snapshot.invites.last_error, None);

    assert!(
        store
            .apply_worker_report(
                "nightshift-vrchat",
                WorkerReport::VrchatRuntime {
                    connected: true,
                    display_name: Some("Hydra Employee".into()),
                    status: VrchatStatus::AskMe,
                    last_invite_action: None,
                    invite_error: Some("network failure".into()),
                },
            )
            .await
    );
    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.invites.last_action.as_ref().map(|action| action.kind), Some(InviteActionKind::Accepted));
    assert_eq!(snapshot.invites.last_error.as_deref(), Some("network failure"));

    assert!(
        store
            .apply_worker_report(
                "nightshift-vrchat",
                WorkerReport::VrchatRuntime {
                    connected: true,
                    display_name: Some("Hydra Employee".into()),
                    status: VrchatStatus::AskMe,
                    last_invite_action: None,
                    invite_error: None,
                },
            )
            .await
    );
    let (snapshot, _) = store.snapshot_and_subscribe().await;
    assert_eq!(snapshot.invites.last_error.as_deref(), Some("network failure"));
}
