use nightshift_protocol::{
    OscState, OverlaySpawnMode, OverlayState, SettingsState, SleepState, SleepingPose, VrchatStatus,
};

#[test]
fn old_overlay_state_defaults_runtime_health() {
    let old_json = r#"{
        "visible": false,
        "spawn_distance_m": 1.0,
        "spawn_mode": "in_front"
    }"#;

    let state: OverlayState = serde_json::from_str(old_json).expect("old overlay state must decode");

    assert!(!state.visible);
    assert_eq!(state.spawn_distance_m, 1.0);
    assert_eq!(state.spawn_mode, OverlaySpawnMode::InFront);
    assert!(!state.runtime_connected);
    assert_eq!(state.runtime_client_version, None);
}

#[test]
fn new_overlay_state_round_trips_runtime_health() {
    let state = OverlayState {
        visible: true,
        spawn_distance_m: 1.25,
        spawn_mode: OverlaySpawnMode::RestoreLastPosition,
        runtime_connected: true,
        runtime_client_version: Some("0.1.0".into()),
    };

    let json = serde_json::to_string(&state).expect("serialize overlay state");
    let decoded: OverlayState = serde_json::from_str(&json).expect("deserialize overlay state");

    assert_eq!(decoded, state);
}

#[test]
fn old_settings_state_defaults_gogo_automation_off() {
    let old_json = r#"{
        "fade_duration_seconds": 3.0,
        "sleep_vrchat_status": "ask_me",
        "wake_vrchat_status": "active"
    }"#;

    let state: SettingsState = serde_json::from_str(old_json).expect("old settings state must decode");

    assert_eq!(state.fade_duration_seconds, 3.0);
    assert_eq!(state.sleep_vrchat_status, VrchatStatus::AskMe);
    assert_eq!(state.wake_vrchat_status, VrchatStatus::Active);
    assert!(!state.gogo_automation_enabled);
}

#[test]
fn old_osc_state_defaults_runtime_details() {
    let old_json = r#"{
        "enabled": true,
        "connected": false
    }"#;

    let state: OscState = serde_json::from_str(old_json).expect("old OSC state must decode");

    assert!(state.enabled);
    assert!(!state.connected);
    assert_eq!(state.avatar_id, None);
    assert_eq!(state.avatar_parameter_count, 0);
    assert!(!state.gogo_detected);
}

#[test]
fn old_sleep_state_defaults_pose_to_unknown() {
    let old_json = r#"{
        "phase": "sleeping",
        "detection_enabled": false,
        "detected_by_hr": false
    }"#;

    let state: SleepState = serde_json::from_str(old_json).expect("old sleep state must decode");

    assert_eq!(state.pose, SleepingPose::Unknown);
}
