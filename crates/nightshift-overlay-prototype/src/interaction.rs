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

// Keep interaction regions aligned with the vertical home dashboard in raster.rs.
// Moon/sun adjust brightness; the single bottom action changes between sleep and wake.
const BRIGHTNESS_DOWN: ButtonRegion =
    region(PanelAction::BrightnessDown, 60.0, 116.0, 340.0, 406.0);
const BRIGHTNESS_UP: ButtonRegion = region(PanelAction::BrightnessUp, 652.0, 708.0, 340.0, 406.0);
const PRIMARY_ACTION: ButtonRegion = region(PanelAction::EnterSleep, 40.0, 728.0, 840.0, 914.0);

const fn region(
    action: PanelAction,
    x_min: f32,
    x_max: f32,
    y_min: f32,
    y_max: f32,
) -> ButtonRegion {
    ButtonRegion {
        action,
        x_min: x_min / 768.0,
        x_max: x_max / 768.0,
        y_min: y_min / 960.0,
        y_max: y_max / 960.0,
    }
}

pub fn action_at_uv(uv: PanelUv) -> Option<PanelAction> {
    action_at_uv_for_sleep_state(uv, false)
}

pub fn action_at_uv_for_sleep_state(uv: PanelUv, wake_action: bool) -> Option<PanelAction> {
    if !uv.u.is_finite() || !uv.v.is_finite() {
        return None;
    }
    for button in [BRIGHTNESS_DOWN, BRIGHTNESS_UP] {
        if inside(uv, button) {
            return Some(button.action);
        }
    }
    if inside(uv, PRIMARY_ACTION) {
        return Some(if wake_action {
            PanelAction::Wake
        } else {
            PanelAction::EnterSleep
        });
    }
    None
}

fn inside(uv: PanelUv, button: ButtonRegion) -> bool {
    uv.u >= button.x_min && uv.u <= button.x_max && uv.v >= button.y_min && uv.v <= button.y_max
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
