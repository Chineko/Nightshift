#[path = "../src/interaction.rs"]
mod interaction;
#[path = "../src/placement.rs"]
mod placement;

use interaction::{
    PanelAction, PanelRay, PanelUv, action_at_uv, begin_panel_grab, hit_test_panel,
    hit_test_panel_with_distance, update_panel_grab,
};
use placement::PanelPose;

const WIDTH: f32 = 0.72;
const HEIGHT: f32 = 0.42;

fn identity_panel() -> PanelPose {
    PanelPose {
        position: [0.0, 0.0, -1.0],
        orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
    }
}

fn controller(position: [f32; 3], orientation_xyzw: [f32; 4]) -> PanelPose {
    PanelPose {
        position,
        orientation_xyzw,
    }
}

#[test]
fn forward_ray_hits_panel_center() {
    let ray = PanelRay {
        origin: [0.0, 0.0, 0.0],
        direction: [0.0, 0.0, -1.0],
    };
    let hit =
        hit_test_panel(ray, identity_panel(), WIDTH, HEIGHT).expect("center ray should hit panel");
    assert!((hit.u - 0.5).abs() < 1.0e-5);
    assert!((hit.v - 0.5).abs() < 1.0e-5);
    let detailed = hit_test_panel_with_distance(ray, identity_panel(), WIDTH, HEIGHT).unwrap();
    assert!((detailed.distance_m - 1.0).abs() < 1.0e-5);
}

#[test]
fn rays_that_miss_or_point_away_do_not_snap_to_panel() {
    assert!(
        hit_test_panel(
            PanelRay {
                origin: [0.0; 3],
                direction: [1.0, 0.0, -1.0]
            },
            identity_panel(),
            WIDTH,
            HEIGHT
        )
        .is_none()
    );
    assert!(
        hit_test_panel(
            PanelRay {
                origin: [0.0; 3],
                direction: [0.0, 0.0, 1.0]
            },
            identity_panel(),
            WIDTH,
            HEIGHT
        )
        .is_none()
    );
}

#[test]
fn invalid_geometry_fails_closed() {
    assert!(
        hit_test_panel(
            PanelRay {
                origin: [f32::NAN, 0.0, 0.0],
                direction: [0.0, 0.0, -1.0]
            },
            identity_panel(),
            WIDTH,
            HEIGHT
        )
        .is_none()
    );
    assert!(
        hit_test_panel(
            PanelRay {
                origin: [0.0; 3],
                direction: [0.0, 0.0, -1.0]
            },
            PanelPose {
                position: [0.0, 0.0, -1.0],
                orientation_xyzw: [0.0; 4]
            },
            WIDTH,
            HEIGHT
        )
        .is_none()
    );
}

#[test]
fn button_regions_map_to_expected_actions_and_gaps_are_dead() {
    let cases = [
        (PanelUv { u: 0.15, v: 0.91 }, PanelAction::EnterSleep),
        (PanelUv { u: 0.38, v: 0.91 }, PanelAction::Wake),
        (PanelUv { u: 0.62, v: 0.91 }, PanelAction::BrightnessDown),
        (PanelUv { u: 0.85, v: 0.91 }, PanelAction::BrightnessUp),
    ];
    for (uv, action) in cases {
        assert_eq!(action_at_uv(uv), Some(action));
    }
    assert_eq!(action_at_uv(PanelUv { u: 0.27, v: 0.91 }), None);
    assert_eq!(action_at_uv(PanelUv { u: 0.5, v: 0.5 }), None);
}

#[test]
fn grab_preserves_controller_relative_translation() {
    let initial_panel = identity_panel();
    let initial_controller = controller([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let grab = begin_panel_grab(initial_controller, initial_panel).unwrap();
    let moved =
        update_panel_grab(grab, controller([0.25, 0.10, 0.0], [0.0, 0.0, 0.0, 1.0])).unwrap();
    assert!((moved.position[0] - 0.25).abs() < 1.0e-5);
    assert!((moved.position[1] - 0.10).abs() < 1.0e-5);
    assert!((moved.position[2] + 1.0).abs() < 1.0e-5);
}

#[test]
fn grab_rotates_panel_with_controller() {
    let initial_controller = controller([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let initial_panel = PanelPose {
        position: [0.0, 0.0, -1.0],
        orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
    };
    let grab = begin_panel_grab(initial_controller, initial_panel).unwrap();
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let moved = update_panel_grab(grab, controller([0.0, 0.0, 0.0], [0.0, s, 0.0, s])).unwrap();
    assert!((moved.position[0] + 1.0).abs() < 1.0e-4);
    assert!(moved.position[2].abs() < 1.0e-4);
    assert!((moved.orientation_xyzw[1] - s).abs() < 1.0e-4);
    assert!((moved.orientation_xyzw[3] - s).abs() < 1.0e-4);
}

#[test]
fn invalid_controller_tracking_rejects_grab_update() {
    let grab =
        begin_panel_grab(controller([0.0; 3], [0.0, 0.0, 0.0, 1.0]), identity_panel()).unwrap();
    assert!(
        update_panel_grab(grab, controller([f32::NAN, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0])).is_none()
    );
}
