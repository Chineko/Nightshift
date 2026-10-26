#[cfg(not(test))]
use crate::controller_input::{begin_slider_drag_for_pressed_hand, slider_drag_active};
#[cfg(test)]
fn begin_slider_drag_for_pressed_hand() -> bool {
    false
}
#[cfg(test)]
const fn slider_drag_active() -> bool {
    false
}

use crate::{
    home_layout::{
        HomeCompositionLayout, HomePrimaryLayout, PanelNavLayout, SettingsToggleLayout,
        SettingsValueLayout, SocialRemoveLayout, SocialTabsLayout, contains,
    },
    layout::LayoutRect,
    placement::{PanelPose, normalize_quaternion_xyzw},
};

const EPSILON: f32 = 1.0e-5;
const PANEL_WIDTH_PX: f32 = 768.0;
const PANEL_HEIGHT_PX: f32 = 960.0;
const BRIGHTNESS_SLIDER_HIT_PAD_Y: f32 = 22.0;
const MIN_BRIGHTNESS_PERCENT: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelRay {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelUv {
    pub u: f32,
    pub v: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelHit {
    pub uv: PanelUv,
    pub distance_m: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelGrab {
    relative_position: [f32; 3],
    relative_orientation_xyzw: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelPage {
    #[default]
    Home,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelAction {
    NavigateHome,
    NavigateSettings,
    EnterSleep,
    Wake,
    SetBrightness(u8),
    ToggleHeartRate,
    ToggleOsc,
    CycleInviteProfile,
    CycleVrchatStatus,
    SelectWhitelist,
    SelectBlacklist,
    RemoveSocial(u8),
}

pub fn action_at_uv(uv: PanelUv) -> Option<PanelAction> {
    action_at_uv_for_page(uv, PanelPage::Home, false)
}

pub fn action_at_uv_for_sleep_state(uv: PanelUv, wake_action: bool) -> Option<PanelAction> {
    action_at_uv_for_page(uv, PanelPage::Home, wake_action)
}

pub fn action_at_uv_for_page(
    uv: PanelUv,
    page: PanelPage,
    wake_action: bool,
) -> Option<PanelAction> {
    if !uv.u.is_finite() || !uv.v.is_finite() {
        return None;
    }

    let x = uv.u * PANEL_WIDTH_PX;
    let y = uv.v * PANEL_HEIGHT_PX;

    // Once a trigger press starts on the slider, keep ownership until release. During
    // that drag the slider wins over every other control, so a wide VR hand motion
    // cannot accidentally navigate or press the giant sleep button.
    if page == PanelPage::Home && slider_drag_active() {
        let track = HomeCompositionLayout::compute().brightness_track;
        return Some(PanelAction::SetBrightness(brightness_percent_at_x(track, x)));
    }

    let navigation = PanelNavLayout::compute();
    if contains(navigation.home, x, y) {
        return Some(PanelAction::NavigateHome);
    }
    if contains(navigation.settings, x, y) {
        return Some(PanelAction::NavigateSettings);
    }

    match page {
        PanelPage::Home => {
            let composition = HomeCompositionLayout::compute();
            let primary = HomePrimaryLayout::compute();
            let slider_hit = brightness_slider_hit_rect(composition.brightness_track);
            if contains(slider_hit, x, y) {
                begin_slider_drag_for_pressed_hand();
                return Some(PanelAction::SetBrightness(brightness_percent_at_x(
                    composition.brightness_track,
                    x,
                )));
            }
            if contains(primary.action, x, y) {
                return Some(if wake_action {
                    PanelAction::Wake
                } else {
                    PanelAction::EnterSleep
                });
            }
        }
        PanelPage::Settings => {
            let social_tabs = SocialTabsLayout::compute();
            if contains(social_tabs.whitelist, x, y) {
                return Some(PanelAction::SelectWhitelist);
            }
            if contains(social_tabs.blacklist, x, y) {
                return Some(PanelAction::SelectBlacklist);
            }

            let toggles = SettingsToggleLayout::compute();
            if contains(toggles.heart_rate_hit(), x, y) {
                return Some(PanelAction::ToggleHeartRate);
            }
            if contains(toggles.osc_hit(), x, y) {
                return Some(PanelAction::ToggleOsc);
            }

            let values = SettingsValueLayout::compute();
            if contains(values.invite_rules, x, y) {
                return Some(PanelAction::CycleInviteProfile);
            }
            if contains(values.vrchat_status, x, y) {
                return Some(PanelAction::CycleVrchatStatus);
            }

            let social_remove = SocialRemoveLayout::compute();
            for index in 0..social_remove.buttons.len() {
                if social_remove
                    .hit(index)
                    .is_some_and(|rect| contains(rect, x, y))
                {
                    return Some(PanelAction::RemoveSocial(index as u8));
                }
            }
        }
    }
    None
}

fn brightness_slider_hit_rect(track: LayoutRect) -> LayoutRect {
    LayoutRect {
        x: track.x,
        y: track.y - BRIGHTNESS_SLIDER_HIT_PAD_Y,
        width: track.width,
        height: track.height + BRIGHTNESS_SLIDER_HIT_PAD_Y * 2.0,
    }
}

fn brightness_percent_at_x(track: LayoutRect, x: f32) -> u8 {
    let normalized = ((x - track.x) / track.width).clamp(0.0, 1.0);
    ((normalized * 100.0).round() as u8).clamp(MIN_BRIGHTNESS_PERCENT, 100)
}

pub fn begin_panel_grab(controller_pose: PanelPose, panel_pose: PanelPose) -> Option<PanelGrab> {
    let controller_orientation = normalize_quaternion_xyzw(controller_pose.orientation_xyzw)?;
    let panel_orientation = normalize_quaternion_xyzw(panel_pose.orientation_xyzw)?;
    if !controller_pose
        .position
        .iter()
        .all(|value| value.is_finite())
        || !panel_pose.position.iter().all(|value| value.is_finite())
    {
        return None;
    }
    let inverse_controller = conjugate(controller_orientation);
    Some(PanelGrab {
        relative_position: rotate(
            subtract(panel_pose.position, controller_pose.position),
            inverse_controller,
        ),
        relative_orientation_xyzw: normalize_quaternion_xyzw(quaternion_multiply(
            inverse_controller,
            panel_orientation,
        ))?,
    })
}

pub fn update_panel_grab(grab: PanelGrab, controller_pose: PanelPose) -> Option<PanelPose> {
    let controller_orientation = normalize_quaternion_xyzw(controller_pose.orientation_xyzw)?;
    if !controller_pose
        .position
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let orientation_xyzw = normalize_quaternion_xyzw(quaternion_multiply(
        controller_orientation,
        grab.relative_orientation_xyzw,
    ))?;
    let position = add(
        controller_pose.position,
        rotate(grab.relative_position, controller_orientation),
    );
    position
        .iter()
        .all(|value| value.is_finite())
        .then_some(PanelPose {
            position,
            orientation_xyzw,
        })
}

pub fn hit_test_panel(
    ray: PanelRay,
    panel_pose: PanelPose,
    panel_width_m: f32,
    panel_height_m: f32,
) -> Option<PanelUv> {
    hit_test_panel_with_distance(ray, panel_pose, panel_width_m, panel_height_m).map(|hit| hit.uv)
}

pub fn hit_test_panel_with_distance(
    ray: PanelRay,
    panel_pose: PanelPose,
    panel_width_m: f32,
    panel_height_m: f32,
) -> Option<PanelHit> {
    if !ray.origin.iter().all(|value| value.is_finite())
        || !ray.direction.iter().all(|value| value.is_finite())
        || !panel_pose.position.iter().all(|value| value.is_finite())
        || !panel_width_m.is_finite()
        || !panel_height_m.is_finite()
        || panel_width_m <= EPSILON
        || panel_height_m <= EPSILON
    {
        return None;
    }

    let orientation = normalize_quaternion_xyzw(panel_pose.orientation_xyzw)?;
    let inverse = conjugate(orientation);
    let local_origin = rotate(subtract(ray.origin, panel_pose.position), inverse);
    let local_direction = rotate(ray.direction, inverse);

    if local_direction[2].abs() <= EPSILON {
        return None;
    }

    let distance = -local_origin[2] / local_direction[2];
    if !distance.is_finite() || distance < 0.0 {
        return None;
    }

    let x = local_origin[0] + local_direction[0] * distance;
    let y = local_origin[1] + local_direction[1] * distance;
    let half_width = panel_width_m * 0.5;
    let half_height = panel_height_m * 0.5;
    if x < -half_width || x > half_width || y < -half_height || y > half_height {
        return None;
    }

    let world_direction = normalize_direction(ray.direction)?;
    let world_hit = add(
        ray.origin,
        scale(world_direction, distance * vector_length(ray.direction)?),
    );
    let world_distance = vector_length(subtract(world_hit, ray.origin))?;
    Some(PanelHit {
        uv: PanelUv {
            u: x / panel_width_m + 0.5,
            v: 0.5 - y / panel_height_m,
        },
        distance_m: world_distance,
    })
}

fn add(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn subtract(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(vector: [f32; 3], factor: f32) -> [f32; 3] {
    [vector[0] * factor, vector[1] * factor, vector[2] * factor]
}

fn conjugate(quaternion_xyzw: [f32; 4]) -> [f32; 4] {
    [
        -quaternion_xyzw[0],
        -quaternion_xyzw[1],
        -quaternion_xyzw[2],
        quaternion_xyzw[3],
    ]
}

fn quaternion_multiply(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = left;
    let [bx, by, bz, bw] = right;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn vector_length(vector: [f32; 3]) -> Option<f32> {
    let length_sq = vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2];
    if !length_sq.is_finite() || length_sq <= EPSILON * EPSILON {
        return None;
    }
    Some(length_sq.sqrt())
}

fn normalize_direction(direction: [f32; 3]) -> Option<[f32; 3]> {
    let length = vector_length(direction)?;
    Some(scale(direction, length.recip()))
}

fn rotate(vector: [f32; 3], quaternion_xyzw: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = quaternion_xyzw;
    let q_vector = [x, y, z];
    let twice_cross = [
        2.0 * (q_vector[1] * vector[2] - q_vector[2] * vector[1]),
        2.0 * (q_vector[2] * vector[0] - q_vector[0] * vector[2]),
        2.0 * (q_vector[0] * vector[1] - q_vector[1] * vector[0]),
    ];
    let cross_q_twice_cross = [
        q_vector[1] * twice_cross[2] - q_vector[2] * twice_cross[1],
        q_vector[2] * twice_cross[0] - q_vector[0] * twice_cross[2],
        q_vector[0] * twice_cross[1] - q_vector[1] * twice_cross[0],
    ];
    [
        vector[0] + w * twice_cross[0] + cross_q_twice_cross[0],
        vector[1] + w * twice_cross[1] + cross_q_twice_cross[1],
        vector[2] + w * twice_cross[2] + cross_q_twice_cross[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_slider_maps_track_to_safe_percent_range() {
        let track = HomeCompositionLayout::compute().brightness_track;
        assert_eq!(brightness_percent_at_x(track, track.x), 2);
        assert_eq!(
            brightness_percent_at_x(track, track.x + track.width * 0.5),
            50
        );
        assert_eq!(brightness_percent_at_x(track, track.x + track.width), 100);
    }

    #[test]
    fn brightness_track_is_directly_interactive() {
        let track = HomeCompositionLayout::compute().brightness_track;
        let uv = PanelUv {
            u: (track.x + track.width * 0.75) / PANEL_WIDTH_PX,
            v: (track.y + track.height * 0.5) / PANEL_HEIGHT_PX,
        };
        assert_eq!(action_at_uv(uv), Some(PanelAction::SetBrightness(75)));
    }
}
