use nightshift_protocol::{
    AppState, HeartRateState, HrProviderStatus, InviteActionKind, InviteRuleSettings, InviteState,
    OscState, OverlaySpawnMode, OverlayState, SettingsState, SleepState, SleepingPose, UiCommand,
    VrchatStatus,
};

#[test]
fn overlay_setting_commands_round_trip_on_the_wire() {
    let commands = [
        UiCommand::SetOverlaySpawnDistance(1.5),
        UiCommand::SetOverlaySpawnMode(OverlaySpawnMode::RestoreLastPosition),
    ];

    for command in commands {
        let json = serde_json::to_string(&command).expect("serialize overlay command");
        let decoded: UiCommand = serde_json::from_str(&json).expect("deserialize overlay command");
        assert_eq!(decoded, command);
    }
}

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
fn old_settings_state_defaults_automations_off() {
    let old_json = r#"{
        "fade_duration_seconds": 3.0,
        "sleep_vrchat_status": "ask_me",
        "wake_vrchat_status": "active"
    }"#;

    let state: SettingsState = serde_json::from_str(old_json).expect("old settings state must decode");

    assert_eq!(state.fade_duration_seconds, 3.0);
    assert_eq!(state.sleep_vrchat_status, VrchatStatus::AskMe);
    assert_eq!(state.wake_vrchat_status, VrchatStatus::Active);
    assert!(!state.vrchat_status_automation_enabled);
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

#[test]
fn old_heart_rate_state_defaults_new_settings_and_health() {
    let old_json = r#"{
        "enabled": true,
        "connected": false,
        "bpm": null,
        "status": "unknown"
    }"#;

    let state: HeartRateState =
        serde_json::from_str(old_json).expect("old heart-rate state must decode");

    assert!(state.enabled);
    assert_eq!(state.device_filter, None);
    assert_eq!(state.connected_device, None);
    assert_eq!(state.provider_status, HrProviderStatus::Unknown);
    assert_eq!(state.last_error, None);
    assert_eq!(state.threshold_bpm, 60);
    assert_eq!(state.calm_period_seconds, 120);
    assert_eq!(state.cooldown_seconds, 15 * 60);
}

#[test]
fn old_invite_state_defaults_rule_settings() {
    let old_json = r#"{
        "enabled": true,
        "profile": "friends",
        "last_action": null
    }"#;

    let state: InviteState = serde_json::from_str(old_json).expect("old invite state must decode");

    assert!(state.enabled);
    assert!(state.rules.whitelist.is_empty());
    assert!(state.rules.accept_friends);
    assert!(state.rules.accept_favorites);
    assert!(state.rules.accept_whitelist);
    assert_eq!(state.rules.fallback, InviteActionKind::Ignored);
}

#[test]
fn unsafe_invite_fallbacks_fail_closed_during_deserialization() {
    for fallback in ["accepted", "asked"] {
        let json = format!(r#"{{"fallback":"{fallback}"}}"#);
        let rules: InviteRuleSettings =
            serde_json::from_str(&json).expect("invite rules must decode safely");
        assert_eq!(rules.fallback, InviteActionKind::Ignored);
    }

    let rules: InviteRuleSettings =
        serde_json::from_str(r#"{"fallback":"declined"}"#).expect("decline fallback must decode");
    assert_eq!(rules.fallback, InviteActionKind::Declined);
}

#[test]
fn old_full_app_state_decodes_with_additive_defaults() {
    let old_json = r#"{
        "connection": "connected",
        "sleep": {
            "phase": "awake",
            "detection_enabled": false,
            "detected_by_hr": false
        },
        "brightness": {
            "current": 1.0,
            "target": 1.0,
            "sleep_target": 0.06,
            "transitioning": false,
            "manual_override": false
        },
        "heart_rate": {
            "enabled": false,
            "connected": false,
            "bpm": null,
            "status": "unknown"
        },
        "osc": {
            "enabled": false,
            "connected": false
        },
        "vrchat": {
            "connected": false,
            "display_name": null,
            "status": "offline"
        },
        "invites": {
            "enabled": false,
            "profile": "disabled",
            "last_action": null
        },
        "overlay": {
            "visible": false,
            "spawn_distance_m": 1.0,
            "spawn_mode": "in_front"
        },
        "settings": {
            "fade_duration_seconds": 3.0,
            "sleep_vrchat_status": "ask_me",
            "wake_vrchat_status": "active"
        }
    }"#;

    let state: AppState = serde_json::from_str(old_json).expect("old full app state must decode");

    assert_eq!(state.sleep.pose, SleepingPose::Unknown);
    assert_eq!(state.heart_rate.device_filter, None);
    assert_eq!(state.heart_rate.connected_device, None);
    assert_eq!(state.heart_rate.provider_status, HrProviderStatus::Unknown);
    assert_eq!(state.heart_rate.last_error, None);
    assert_eq!(state.heart_rate.threshold_bpm, 60);
    assert_eq!(state.heart_rate.calm_period_seconds, 120);
    assert_eq!(state.heart_rate.cooldown_seconds, 15 * 60);
    assert_eq!(state.osc.avatar_id, None);
    assert_eq!(state.osc.avatar_parameter_count, 0);
    assert!(!state.osc.gogo_detected);
    assert!(state.invites.rules.whitelist.is_empty());
    assert_eq!(state.invites.rules.fallback, InviteActionKind::Ignored);
    assert!(!state.overlay.runtime_connected);
    assert_eq!(state.overlay.runtime_client_version, None);
    assert!(!state.settings.vrchat_status_automation_enabled);
    assert!(!state.settings.gogo_automation_enabled);
}
