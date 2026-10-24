#[path = "../src/interaction.rs"]
mod interaction;
#[path = "../src/placement.rs"]
mod placement;

use interaction::{
    PanelAction, PanelPage, PanelRay, PanelUv, action_at_uv, action_at_uv_for_page,
    action_at_uv_for_sleep_state, begin_panel_grab,
    hit_test_panel, hit_test_panel_with_distance, update_panel_grab,
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
fn dashboard_regions_map_to_brightness_and_stateful_primary_action() {
    assert_eq!(
        action_at_uv(PanelUv { u: 0.11, v: 0.39 }),
        Some(PanelAction::BrightnessDown)
    );
    assert_eq!(
        action_at_uv(PanelUv { u: 0.89, v: 0.39 }),
        Some(PanelAction::BrightnessUp)
    );

    let primary = PanelUv { u: 0.5, v: 0.91 };
    assert_eq!(
        action_at_uv_for_sleep_state(primary, false),
        Some(PanelAction::EnterSleep)
    );
    assert_eq!(
        action_at_uv_for_sleep_state(primary, true),
        Some(PanelAction::Wake)
    );

    assert_eq!(action_at_uv(PanelUv { u: 0.5, v: 0.5 }), None);
}

#[test]
fn settings_regions_map_to_navigation_quick_controls_and_social_remove() {
    assert_eq!(
        action_at_uv_for_page(PanelUv { u: 0.86, v: 0.065 }, PanelPage::Home, false),
        Some(PanelAction::NavigateSettings)
    );
    assert_eq!(
        action_at_uv_for_page(PanelUv { u: 0.71, v: 0.065 }, PanelPage::Settings, false),
        Some(PanelAction::NavigateHome)
    );
    assert_eq!(
        action_at_uv_for_page(PanelUv { u: 0.52, v: 0.29 }, PanelPage::Settings, false),
        Some(PanelAction::ToggleHeartRate)
    );
    assert_eq!(
        action_at_uv_for_page(PanelUv { u: 0.90, v: 0.40 }, PanelPage::Settings, false),
        Some(PanelAction::RemoveSocial(0))
    );
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
