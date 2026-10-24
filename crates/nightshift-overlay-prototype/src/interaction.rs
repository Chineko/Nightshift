use crate::placement::{PanelPose, normalize_quaternion_xyzw};

const EPSILON: f32 = 1.0e-5;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelAction {
    EnterSleep,
    Wake,
    BrightnessDown,
    BrightnessUp,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ButtonRegion {
    action: PanelAction,
    x_min: f32,
    x_max: f32,
    y_min: f32,
    y_max: f32,
}

// Keep the interaction regions aligned with the bottom-row buttons rendered by raster.rs.
// The panel is furniture, not a face-hugger: misses stay misses instead of snapping.
const BUTTONS: [ButtonRegion; 4] = [
    button(PanelAction::EnterSleep, 36.0, 201.0),
    button(PanelAction::Wake, 213.0, 378.0),
    button(PanelAction::BrightnessDown, 390.0, 555.0),
    button(PanelAction::BrightnessUp, 567.0, 732.0),
];

const fn button(action: PanelAction, x_min: f32, x_max: f32) -> ButtonRegion {
    ButtonRegion {
        action,
        x_min: x_min / 768.0,
        x_max: x_max / 768.0,
        y_min: 388.0 / 448.0,
        y_max: 432.0 / 448.0,
    }
}

pub fn action_at_uv(uv: PanelUv) -> Option<PanelAction> {
    if !uv.u.is_finite() || !uv.v.is_finite() {
        return None;
    }

    BUTTONS.iter().find_map(|button| {
        (uv.u >= button.x_min
            && uv.u <= button.x_max
            && uv.v >= button.y_min
            && uv.v <= button.y_max)
            .then_some(button.action)
    })
}

pub fn hit_test_panel(
    ray: PanelRay,
    panel_pose: PanelPose,
    panel_width_m: f32,
    panel_height_m: f32,
) -> Option<PanelUv> {
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
    let inverse = [-orientation[0], -orientation[1], -orientation[2], orientation[3]];
    let local_origin = rotate(
        subtract(ray.origin, panel_pose.position),
        inverse,
    );
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

    Some(PanelUv {
        u: x / panel_width_m + 0.5,
        v: 0.5 - y / panel_height_m,
    })
}

fn subtract(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
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
