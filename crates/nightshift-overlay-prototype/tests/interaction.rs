#[path = "../src/placement.rs"]
mod placement;
#[path = "../src/interaction.rs"]
mod interaction;

use interaction::{PanelAction, PanelRay, PanelUv, action_at_uv, hit_test_panel};
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
