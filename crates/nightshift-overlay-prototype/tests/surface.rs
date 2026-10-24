#[path = "../src/placement.rs"]
mod placement;
#[path = "../src/surface.rs"]
mod surface;

use nightshift_protocol::{OverlaySpawnMode, OverlayState};
use placement::PanelPose;
use surface::{SurfaceController, SurfaceTarget};

fn approx_eq(left: f32, right: f32) {
    assert!((left - right).abs() < 1.0e-5, "left={left}, right={right}");
}

fn assert_position(actual: PanelPose, expected: [f32; 3]) {
    for (actual, expected) in actual.position.into_iter().zip(expected) {
        approx_eq(actual, expected);
    }
}

fn target(visible: bool, distance_m: f32, spawn_mode: OverlaySpawnMode) -> SurfaceTarget {
    let state = OverlayState {
        visible,
        spawn_distance_m: distance_m,
        spawn_mode,
        ..OverlayState::default()
    };
    SurfaceTarget::from(&state)
}

#[test]
fn show_edge_captures_world_pose_once_instead_of_following_head() {
    let mut controller = SurfaceController::default();
    let shown = target(true, 1.0, OverlaySpawnMode::InFront);

    let first = controller
        .sync(shown, [0.0, 1.6, 0.0], [0.0, 0.0, 0.0, 1.0])
        .expect("show should capture a pose");
    assert_position(first, [0.0, 1.6, -1.0]);

    let after_head_move = controller
        .sync(shown, [5.0, 1.6, 5.0], [0.0, 0.0, 0.0, 1.0])
        .expect("visible surface keeps its captured pose");
    assert_eq!(after_head_move, first);
}

#[test]
fn hide_removes_active_pose_and_restore_reuses_last_world_pose() {
    let mut controller = SurfaceController::default();
    let first = controller
        .sync(
            target(true, 1.25, OverlaySpawnMode::InFront),
            [1.0, 2.0, 3.0],
            [0.0, 0.0, 0.0, 1.0],
        )
        .expect("first show");

    assert_eq!(
        controller.sync(
            target(false, 1.25, OverlaySpawnMode::InFront),
            [1.0, 2.0, 3.0],
            [0.0, 0.0, 0.0, 1.0],
        ),
        None
    );
    assert_eq!(controller.active_pose(), None);
    assert_eq!(controller.saved_pose(), Some(first));

    let restored = controller
        .sync(
            target(true, 4.0, OverlaySpawnMode::RestoreLastPosition),
            [9.0, 9.0, 9.0],
            [0.0, 0.0, 0.0, 1.0],
        )
        .expect("restore should use saved world pose");
    assert_eq!(restored, first);
}

#[test]
fn restore_without_saved_pose_falls_back_to_in_front() {
    let mut controller = SurfaceController::default();
    let pose = controller
        .sync(
            target(true, 2.0, OverlaySpawnMode::RestoreLastPosition),
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        )
        .expect("restore fallback should spawn");
    assert_position(pose, [0.0, 1.0, -2.0]);
}

#[test]
fn invalid_tracking_does_not_make_surface_visible() {
    let mut controller = SurfaceController::default();
    assert_eq!(
        controller.sync(
            target(true, 1.0, OverlaySpawnMode::InFront),
            [f32::NAN, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ),
        None
    );
    assert_eq!(controller.active_pose(), None);
}

#[test]
fn remembered_pose_is_validated_and_normalized() {
    let mut controller = SurfaceController::default();
    assert!(!controller.remember_pose(PanelPose {
        position: [f32::NAN, 0.0, 0.0],
        orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
    }));
    assert!(!controller.remember_pose(PanelPose {
        position: [0.0, 0.0, 0.0],
        orientation_xyzw: [0.0, 0.0, 0.0, 0.0],
    }));
    assert!(controller.remember_pose(PanelPose {
        position: [1.0, 2.0, 3.0],
        orientation_xyzw: [0.0, 0.0, 0.0, 2.0],
    }));
    assert_eq!(
        controller.saved_pose(),
        Some(PanelPose {
            position: [1.0, 2.0, 3.0],
            orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
        })
    );
}

#[test]
fn target_sanitizes_distance_before_runtime_use() {
    assert_eq!(target(true, f32::NAN, OverlaySpawnMode::InFront).distance_m, 1.0);
    assert_eq!(target(true, 0.0, OverlaySpawnMode::InFront).distance_m, 0.25);
    assert_eq!(target(true, 99.0, OverlaySpawnMode::InFront).distance_m, 5.0);
}
