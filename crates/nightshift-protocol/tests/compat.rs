use nightshift_protocol::{OverlaySpawnMode, OverlayState};

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
