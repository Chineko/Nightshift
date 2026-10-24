use nightshift_protocol::{OverlaySpawnMode, OverlayState};

use crate::placement::{PanelPose, normalize_quaternion_xyzw, panel_pose_in_front, sanitize_panel_distance};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceTarget {
    pub visible: bool,
    pub distance_m: f32,
    pub spawn_mode: OverlaySpawnMode,
}

impl From<&OverlayState> for SurfaceTarget {
    fn from(state: &OverlayState) -> Self {
        Self {
            visible: state.visible,
            distance_m: sanitize_panel_distance(state.spawn_distance_m),
            spawn_mode: state.spawn_mode,
        }
    }
}

fn sanitize_pose(pose: PanelPose) -> Option<PanelPose> {
    if !pose.position.iter().all(|component| component.is_finite()) {
        return None;
    }
    Some(PanelPose {
        position: pose.position,
        orientation_xyzw: normalize_quaternion_xyzw(pose.orientation_xyzw)?,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceController {
    visible: bool,
    active_pose: Option<PanelPose>,
    saved_pose: Option<PanelPose>,
}

impl SurfaceController {
    pub const fn active_pose(&self) -> Option<PanelPose> {
        self.active_pose
    }

    pub const fn saved_pose(&self) -> Option<PanelPose> {
        self.saved_pose
    }

    pub fn remember_pose(&mut self, pose: PanelPose) -> bool {
        let Some(pose) = sanitize_pose(pose) else {
            return false;
        };
        self.saved_pose = Some(pose);
        if self.visible {
            self.active_pose = Some(pose);
        }
        true
    }

    pub fn sync(
        &mut self,
        target: SurfaceTarget,
        head_position: [f32; 3],
        head_orientation_xyzw: [f32; 4],
    ) -> Option<PanelPose> {
        if !target.visible {
            if self.visible {
                self.saved_pose = self.active_pose.and_then(sanitize_pose).or(self.saved_pose);
            }
            self.visible = false;
            self.active_pose = None;
            return None;
        }

        // Placement is captured only on the hidden -> visible edge. This keeps the panel
        // world-locked instead of accidentally turning an "in front" spawn into a head-locked HUD.
        if self.visible {
            return self.active_pose;
        }

        let pose = match target.spawn_mode {
            OverlaySpawnMode::RestoreLastPosition => self.saved_pose.and_then(sanitize_pose).or_else(|| {
                panel_pose_in_front(head_position, head_orientation_xyzw, target.distance_m)
            }),
            OverlaySpawnMode::InFront => {
                panel_pose_in_front(head_position, head_orientation_xyzw, target.distance_m)
            }
        };

        self.visible = pose.is_some();
        self.active_pose = pose;
        if pose.is_some() {
            self.saved_pose = pose;
        }
        pose
    }
}
