#[path = "../src/placement.rs"]
mod placement;

use placement::{
    MAX_PANEL_DISTANCE_M, MIN_PANEL_DISTANCE_M, PanelPose, panel_pose_in_front,
    sanitize_panel_distance,
};

fn approx_eq(left: f32, right: f32) {
    assert!((left - right).abs() < 1.0e-5, "left={left}, right={right}");
}

fn assert_pose(actual: PanelPose, position: [f32; 3], orientation: [f32; 4]) {
    for (actual, expected) in actual.position.into_iter().zip(position) {
        approx_eq(actual, expected);
    }
    for (actual, expected) in actual.orientation_xyzw.into_iter().zip(orientation) {
        approx_eq(actual, expected);
    }
}

#[test]
fn identity_head_pose_places_panel_on_negative_z_and_faces_viewer() {
    let pose = panel_pose_in_front([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0], 1.5)
        .expect("valid head pose");

    assert_pose(pose, [1.0, 2.0, 1.5], [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn head_yaw_rotates_spawn_direction_with_the_viewer() {
    let half_sqrt_two = std::f32::consts::FRAC_1_SQRT_2;
    let pose = panel_pose_in_front(
        [0.0, 0.0, 0.0],
        [0.0, half_sqrt_two, 0.0, half_sqrt_two],
        2.0,
    )
    .expect("valid yaw pose");

    assert_pose(
        pose,
        [-2.0, 0.0, 0.0],
        [0.0, half_sqrt_two, 0.0, half_sqrt_two],
    );
}

#[test]
fn placement_normalizes_valid_head_orientation() {
    let pose = panel_pose_in_front([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 2.0], 1.0)
        .expect("normalizable orientation");

    assert_pose(pose, [0.0, 0.0, -1.0], [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn placement_rejects_invalid_tracking_pose() {
    assert!(panel_pose_in_front([f32::NAN, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], 1.0).is_none());
    assert!(panel_pose_in_front([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], 1.0).is_none());
    assert!(panel_pose_in_front([0.0, 0.0, 0.0], [0.0, f32::INFINITY, 0.0, 1.0], 1.0).is_none());
}

#[test]
fn placement_distance_is_defensively_bounded() {
    approx_eq(sanitize_panel_distance(0.0), MIN_PANEL_DISTANCE_M);
    approx_eq(sanitize_panel_distance(99.0), MAX_PANEL_DISTANCE_M);
    approx_eq(sanitize_panel_distance(f32::NAN), 1.0);
}
