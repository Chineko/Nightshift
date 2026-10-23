// Sleeping-pose geometry is adapted from OyasumiVR by Raphiiko (MIT).
// See THIRD_PARTY_NOTICES.md for the full attribution and license text.

const SAMPLE_INTERVAL_MS: u64 = 250;
const WINDOW_MS: u64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepingPose {
    Unknown,
    Front,
    Back,
    Left,
    Right,
}

const POSE_PRIORITY: [SleepingPose; 4] = [
    SleepingPose::Right,
    SleepingPose::Left,
    SleepingPose::Back,
    SleepingPose::Front,
];

struct SenderSpec {
    size: [f64; 3],
    offset: [f64; 3],
}

const MAIN_AXIS_SENDER: SenderSpec = SenderSpec {
    size: [2.0, 2.0, 15.0],
    offset: [0.0, 0.0, 7.5],
};
const CROSS_AXIS_SENDER: SenderSpec = SenderSpec {
    size: [15.0, 4.0, 4.0],
    offset: [-7.5, 0.0, 0.0],
};

const RECEIVERS: [(SleepingPose, f64, [f64; 3]); 9] = [
    (SleepingPose::Front, 3.0, [0.0, 3.5, 10.0]),
    (SleepingPose::Back, 3.0, [0.0, -8.0, 5.5]),
    (SleepingPose::Back, 3.0, [0.0, -8.0, 7.0]),
    (SleepingPose::Right, 4.0, [0.0, 9.0, 1.0]),
    (SleepingPose::Right, 4.0, [0.0, 9.0, 0.0]),
    (SleepingPose::Right, 4.0, [0.0, 9.0, -1.0]),
    (SleepingPose::Left, 4.0, [0.0, -10.0, 1.0]),
    (SleepingPose::Left, 4.0, [0.0, -10.0, 0.0]),
    (SleepingPose::Left, 4.0, [0.0, -10.0, -1.0]),
];

#[derive(Clone, Debug)]
struct Obb {
    center: [f64; 3],
    half_size: [f64; 3],
    axes: [[f64; 3]; 3],
}

#[derive(Clone, Debug)]
struct Receiver {
    pose: SleepingPose,
    center: [f64; 3],
    radius: f64,
    colliding: bool,
}

#[derive(Clone, Debug)]
pub struct PoseGeometry {
    main_axis_sender: Obb,
    cross_axis_sender: Obb,
    receivers: Vec<Receiver>,
}

impl PoseGeometry {
    pub fn new(quaternion: [f64; 4]) -> Self {
        let sender_rotation = compose(quaternion);
        let main_axis_sender = sender(&sender_rotation, &MAIN_AXIS_SENDER);
        let cross_axis_sender = sender(&sender_rotation, &CROSS_AXIS_SENDER);
        let receiver_rotation = compose(yaw_only(&sender_rotation));
        let receivers = RECEIVERS
            .iter()
            .map(|&(pose, radius, offset)| {
                let center = translation(&receiver_rotation, offset);
                let sender = match pose {
                    SleepingPose::Front | SleepingPose::Back => &main_axis_sender,
                    _ => &cross_axis_sender,
                };
                Receiver {
                    pose,
                    center,
                    radius,
                    colliding: intersects_sphere(sender, center, radius),
                }
            })
            .collect();

        Self {
            main_axis_sender,
            cross_axis_sender,
            receivers,
        }
    }

    pub fn active_pose(&self) -> Option<SleepingPose> {
        POSE_PRIORITY
            .into_iter()
            .find(|&pose| self.receivers.iter().any(|receiver| receiver.pose == pose && receiver.colliding))
    }

    pub fn sender_centers(&self) -> ([f64; 3], [f64; 3]) {
        (self.main_axis_sender.center, self.cross_axis_sender.center)
    }

    pub fn receiver_count(&self) -> usize {
        self.receivers.len()
    }
}

pub fn classify(geometry: &PoseGeometry, previous: SleepingPose) -> SleepingPose {
    geometry.active_pose().unwrap_or(previous)
}

pub struct PoseTracker {
    raw_pose: SleepingPose,
    window: Vec<SleepingPose>,
    window_end_ms: u64,
    next_sample_ms: u64,
    stable_pose: SleepingPose,
}

impl PoseTracker {
    pub fn new(start_ms: u64) -> Self {
        Self {
            raw_pose: classify(
                &PoseGeometry::new([0.0, 0.0, 0.0, 1.0]),
                SleepingPose::Unknown,
            ),
            window: Vec::new(),
            window_end_ms: start_ms + WINDOW_MS,
            next_sample_ms: 0,
            stable_pose: SleepingPose::Unknown,
        }
    }

    pub fn advance(&mut self, now_ms: u64) -> Option<SleepingPose> {
        let mut changed = None;
        while now_ms >= self.window_end_ms {
            let window = std::mem::take(&mut self.window);
            self.window_end_ms += WINDOW_MS;
            if window.len() >= 2 && window.iter().all(|&pose| pose == window[0]) {
                changed = self.set_stable(window[0]).or(changed);
            }
        }
        changed
    }

    pub fn sample_due(&self, now_ms: u64) -> bool {
        now_ms > self.next_sample_ms
    }

    pub fn sample(&mut self, now_ms: u64, quaternion: [f64; 4]) -> SleepingPose {
        self.next_sample_ms = now_ms + SAMPLE_INTERVAL_MS;
        let geometry = PoseGeometry::new(quaternion);
        self.raw_pose = classify(&geometry, self.raw_pose);
        self.window.push(self.raw_pose);
        self.raw_pose
    }

    pub fn force(&mut self, now_ms: u64, pose: SleepingPose) -> Vec<SleepingPose> {
        let closed = self.advance(now_ms);
        closed.into_iter().chain(self.set_stable(pose)).collect()
    }

    pub fn set_stable(&mut self, pose: SleepingPose) -> Option<SleepingPose> {
        if pose == self.stable_pose {
            return None;
        }
        self.stable_pose = pose;
        Some(pose)
    }

    pub fn stable_pose(&self) -> SleepingPose {
        self.stable_pose
    }
}

fn compose([x, y, z, w]: [f64; 4]) -> [f64; 16] {
    let (x2, y2, z2) = (x + x, y + y, z + z);
    let (xx, xy, xz) = (x * x2, x * y2, x * z2);
    let (yy, yz, zz) = (y * y2, y * z2, z * z2);
    let (wx, wy, wz) = (w * x2, w * y2, w * z2);
    [
        1.0 - (yy + zz),
        xy + wz,
        xz - wy,
        0.0,
        xy - wz,
        1.0 - (xx + zz),
        yz + wx,
        0.0,
        xz + wy,
        yz - wx,
        1.0 - (xx + yy),
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

fn translation(matrix: &[f64; 16], [ox, oy, oz]: [f64; 3]) -> [f64; 3] {
    [
        matrix[0] * ox + matrix[4] * oy + matrix[8] * oz + matrix[12],
        matrix[1] * ox + matrix[5] * oy + matrix[9] * oz + matrix[13],
        matrix[2] * ox + matrix[6] * oy + matrix[10] * oz + matrix[14],
    ]
}

fn yaw_only(matrix: &[f64; 16]) -> [f64; 4] {
    let (m11, m13, m23, m31, m33) = (matrix[0], matrix[8], matrix[9], matrix[2], matrix[10]);
    let yaw = if m23.abs() < 0.9999999 {
        libm::atan2(m13, m33)
    } else {
        libm::atan2(-m31, m11)
    };
    [0.0, libm::sin(yaw / 2.0), 0.0, libm::cos(yaw / 2.0)]
}

fn sender(rotation: &[f64; 16], spec: &SenderSpec) -> Obb {
    let mut axes = [[0.0; 3]; 3];
    let mut half_size = [0.0; 3];
    for (index, axis) in axes.iter_mut().enumerate() {
        let column = [
            rotation[index * 4],
            rotation[index * 4 + 1],
            rotation[index * 4 + 2],
        ];
        let scale = (column[0] * column[0] + column[1] * column[1] + column[2] * column[2]).sqrt();
        let inverse = 1.0 / scale;
        *axis = column.map(|value| value * inverse);
        half_size[index] = spec.size[index] * 0.5 * scale;
    }

    Obb {
        center: translation(rotation, spec.offset),
        half_size,
        axes,
    }
}

fn intersects_sphere(obb: &Obb, center: [f64; 3], radius: f64) -> bool {
    let vector = [
        center[0] - obb.center[0],
        center[1] - obb.center[1],
        center[2] - obb.center[2],
    ];
    let mut closest = obb.center;

    for (axis, half) in obb.axes.iter().zip(obb.half_size) {
        let distance = (vector[0] * axis[0] + vector[1] * axis[1] + vector[2] * axis[2])
            .min(half)
            .max(-half);
        for index in 0..3 {
            closest[index] += axis[index] * distance;
        }
    }

    let delta = [
        closest[0] - center[0],
        closest[1] - center[1],
        closest[2] - center[2],
    ];
    delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2] <= radius * radius
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headset(pitch: f64, roll: f64) -> [f64; 4] {
        let (s1, c1) = (pitch.to_radians() / 2.0).sin_cos();
        let (s3, c3) = (roll.to_radians() / 2.0).sin_cos();
        [s1 * c3, -s1 * s3, c1 * s3, c1 * c3]
    }

    fn pose(pitch: f64, roll: f64) -> Option<SleepingPose> {
        PoseGeometry::new(headset(pitch, roll)).active_pose()
    }

    #[test]
    fn classifies_basic_headset_orientations() {
        assert_eq!(pose(0.0, 0.0), Some(SleepingPose::Front));
        assert_eq!(pose(75.0, 0.0), Some(SleepingPose::Back));
        assert_eq!(pose(0.0, 90.0), Some(SleepingPose::Left));
        assert_eq!(pose(0.0, -90.0), Some(SleepingPose::Right));
    }

    #[test]
    fn yaw_does_not_change_pose() {
        let (s, c) = (50f64.to_radians() / 2.0).sin_cos();
        let [x, y, z, w] = headset(0.0, 90.0);
        let yawed = [c * x + s * z, c * y + s * w, c * z - s * x, c * w - s * y];
        assert_eq!(
            PoseGeometry::new(yawed).active_pose(),
            Some(SleepingPose::Left)
        );
    }

    #[test]
    fn stable_pose_requires_two_agreeing_samples_in_window() {
        let mut tracker = PoseTracker::new(0);
        tracker.sample(250, headset(0.0, 90.0));
        tracker.sample(500, headset(0.0, 90.0));
        assert_eq!(tracker.advance(999), None);
        assert_eq!(tracker.advance(1000), Some(SleepingPose::Left));
        assert_eq!(tracker.stable_pose(), SleepingPose::Left);
    }

    #[test]
    fn mixed_window_does_not_change_stable_pose() {
        let mut tracker = PoseTracker::new(0);
        tracker.sample(250, headset(0.0, 90.0));
        tracker.sample(500, headset(0.0, -90.0));
        assert_eq!(tracker.advance(1000), None);
        assert_eq!(tracker.stable_pose(), SleepingPose::Unknown);
    }
}
