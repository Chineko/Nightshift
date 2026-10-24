#[path = "../src/placement.rs"]
mod placement;
#[path = "../src/panel.rs"]
mod panel;

use panel::panel_pose_to_xr;
use placement::PanelPose;

#[test]
fn panel_pose_conversion_preserves_world_transform() {
    let xr_pose = panel_pose_to_xr(PanelPose {
        position: [1.0, 2.0, -3.0],
        orientation_xyzw: [0.1, 0.2, 0.3, 0.9],
    });

    assert_eq!(xr_pose.position.x, 1.0);
    assert_eq!(xr_pose.position.y, 2.0);
    assert_eq!(xr_pose.position.z, -3.0);
    assert_eq!(xr_pose.orientation.x, 0.1);
    assert_eq!(xr_pose.orientation.y, 0.2);
    assert_eq!(xr_pose.orientation.z, 0.3);
    assert_eq!(xr_pose.orientation.w, 0.9);
}

#[test]
fn panel_surface_dimensions_are_stable_and_positive() {
    assert!(panel::PANEL_TEXTURE_WIDTH > 0);
    assert!(panel::PANEL_TEXTURE_HEIGHT > 0);
    assert!(panel::PANEL_WIDTH_M > 0.0);
    assert!(panel::PANEL_HEIGHT_M > 0.0);
}
