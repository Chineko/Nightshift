#[path = "../src/interaction.rs"]
mod interaction;
#[path = "../src/placement.rs"]
mod placement;

use interaction::{
    PanelAction, PanelRay, PanelUv, action_at_uv, begin_panel_grab, hit_test_panel,
    update_panel_grab,
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

#[test]
fn forward_ray_hits_panel_center() {
    let hit = hit_test_panel(
        PanelRay {
            origin: [0.0, 0.0, 0.0],
            direction: [0.0, 0.0, -1.0],
        },
        identity_panel(),
        WIDTH,
        HEIGHT,
    )
    .expect("center ray should hit panel");

    assert!((hit.u - 0.5).abs() < 1.0e-5);
    assert!((hit.v - 0.5).abs() < 1.0e-5);
}

#[test]
fn rays_that_miss_or_point_away_do_not_snap_to_panel() {
    assert!(
        hit_test_panel(
            PanelRay {
                origin: [0.0, 0.0, 0.0],
                direction: [1.0, 0.0, -1.0],
            },
            identity_panel(),
            WIDTH,
            HEIGHT,
        )
        .is_none()
    );
    assert!(
        hit_test_panel(
            PanelRay {
                origin: [0.0, 0.0, 0.0],
                direction: [0.0, 0.0, 1.0],
            },
            identity_panel(),
            WIDTH,
            HEIGHT,
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
                direction: [0.0, 0.0, -1.0],
            },
            identity_panel(),
            WIDTH,
            HEIGHT,
        )
        .is_none()
    );
    assert!(
        hit_test_panel(
            PanelRay {
                origin: [0.0, 0.0, 0.0],
                direction: [0.0, 0.0, -1.0],
            },
            PanelPose {
                position: [0.0, 0.0, -1.0],
                orientation_xyzw: [0.0; 4],
            },
            WIDTH,
            HEIGHT,
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
fn grab_moves_panel_with_controller_translation_without_rotating_it() {
    let initial = identity_panel();
    let grab = begin_panel_grab(
        PanelRay {
            origin: [0.0, 0.0, 0.0],
            direction: [0.0, 0.0, -1.0],
        },
        initial,
        WIDTH,
        HEIGHT,
    )
    .expect("center press should begin grab");

    let moved = update_panel_grab(
        grab,
        PanelRay {
            origin: [0.25, 0.10, 0.0],
            direction: [0.0, 0.0, -1.0],
        },
    )
    .expect("valid tracked ray should move panel");

    assert!((moved.position[0] - 0.25).abs() < 1.0e-5);
    assert!((moved.position[1] - 0.10).abs() < 1.0e-5);
    assert!((moved.position[2] + 1.0).abs() < 1.0e-5);
    assert_eq!(moved.orientation_xyzw, initial.orientation_xyzw);
}

#[test]
fn off_center_grab_preserves_the_grabbed_point() {
    let initial = identity_panel();
    let start_ray = PanelRay {
        origin: [0.2, 0.0, 0.0],
        direction: [0.0, 0.0, -1.0],
    };
    let grab = begin_panel_grab(start_ray, initial, WIDTH, HEIGHT)
        .expect("off-center panel point should be grabbable");
    let moved = update_panel_grab(
        grab,
        PanelRay {
            origin: [0.35, 0.0, 0.0],
            direction: [0.0, 0.0, -1.0],
        },
    )
    .expect("translated controller should move panel");

    assert!((moved.position[0] - 0.15).abs() < 1.0e-5);
    assert!((moved.position[2] + 1.0).abs() < 1.0e-5);
}

#[test]
fn grab_update_rejects_invalid_tracking() {
    let grab = begin_panel_grab(
        PanelRay {
            origin: [0.0, 0.0, 0.0],
            direction: [0.0, 0.0, -1.0],
        },
        identity_panel(),
        WIDTH,
        HEIGHT,
    )
    .unwrap();
    assert!(
        update_panel_grab(
            grab,
            PanelRay {
                origin: [f32::NAN, 0.0, 0.0],
                direction: [0.0, 0.0, -1.0],
            },
        )
        .is_none()
    );
}
