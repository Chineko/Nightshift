pub const MIN_PANEL_DISTANCE_M: f32 = 0.25;
pub const MAX_PANEL_DISTANCE_M: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelPose {
    pub position: [f32; 3],
    pub orientation_xyzw: [f32; 4],
}

pub fn sanitize_panel_distance(distance_m: f32) -> f32 {
    if distance_m.is_finite() {
        distance_m.clamp(MIN_PANEL_DISTANCE_M, MAX_PANEL_DISTANCE_M)
    } else {
        1.0
    }
}

pub fn normalize_quaternion_xyzw(value: [f32; 4]) -> Option<[f32; 4]> {
    if !value.iter().all(|component| component.is_finite()) {
        return None;
    }

    let length_squared = value.iter().map(|component| component * component).sum::<f32>();
    if !length_squared.is_finite() || length_squared <= f32::EPSILON {
        return None;
    }

    let inverse_length = length_squared.sqrt().recip();
    Some([
        value[0] * inverse_length,
        value[1] * inverse_length,
        value[2] * inverse_length,
        value[3] * inverse_length,
    ])
}

fn rotate_vector_by_quaternion(vector: [f32; 3], quaternion_xyzw: [f32; 4]) -> [f32; 3] {
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

/// Places a world-locked quad in front of the current HMD pose.
///
/// OpenXR uses -Z as viewer-forward, while a quad's visible front-face normal is +Z.
/// Reusing the normalized HMD orientation therefore puts the quad at -Z in front of
/// the viewer while keeping its +Z face pointed back toward the viewer.
pub fn panel_pose_in_front(
    head_position: [f32; 3],
    head_orientation_xyzw: [f32; 4],
    distance_m: f32,
) -> Option<PanelPose> {
    if !head_position.iter().all(|component| component.is_finite()) {
        return None;
    }

    let orientation = normalize_quaternion_xyzw(head_orientation_xyzw)?;
    let distance = sanitize_panel_distance(distance_m);
    let offset = rotate_vector_by_quaternion([0.0, 0.0, -distance], orientation);

    Some(PanelPose {
        position: [
            head_position[0] + offset[0],
            head_position[1] + offset[1],
            head_position[2] + offset[2],
        ],
        orientation_xyzw: orientation,
    })
}
