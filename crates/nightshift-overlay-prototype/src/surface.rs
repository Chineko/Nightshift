use nightshift_protocol::{OverlayPose, OverlaySpawnMode, OverlayState};

use crate::placement::{
    PanelPose, normalize_quaternion_xyzw, panel_pose_in_front, sanitize_panel_distance,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceTarget {
    pub visible: bool,
    pub distance_m: f32,
    pub spawn_mode: OverlaySpawnMode,
    pub remembered_pose: Option<PanelPose>,
}

impl From<&OverlayState> for SurfaceTarget {
    fn from(state: &OverlayState) -> Self {
        Self {
            visible: state.visible,
            distance_m: sanitize_panel_distance(state.spawn_distance_m),
            spawn_mode: state.spawn_mode,
            remembered_pose: state
                .remembered_pose
                .map(panel_pose_from_protocol)
                .and_then(sanitize_pose),
        }
    }
}

pub const fn panel_pose_to_protocol(pose: PanelPose) -> OverlayPose {
    OverlayPose {
        position: pose.position,
        orientation_xyzw: pose.orientation_xyzw,
    }
}

const fn panel_pose_from_protocol(pose: OverlayPose) -> PanelPose {
    PanelPose {
        position: pose.position,
        orientation_xyzw: pose.orientation_xyzw,
    }
}

fn sanitize_pose(pose: PanelPose) -> Option<PanelPose> {
    // Reality occasionally hands us NaNs. Hydra does not get to keep those as souvenirs.
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

    pub fn move_active_pose(&mut self, pose: PanelPose) -> bool {
        if !self.visible {
            return false;
        }
        let Some(pose) = sanitize_pose(pose) else {
            return false;
        };
        self.active_pose = Some(pose);
        true
    }

    pub fn remember_pose(&mut self, pose: PanelPose) -> bool {
        let Some(pose) = sanitize_pose(pose) else {
            return false;
        };
        // Put the shiny thing somewhere safe before a compositor gremlin eats it.
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
                // Hide the panel, not the memory of where we parked it.
                self.saved_pose = self.active_pose.and_then(sanitize_pose).or(self.saved_pose);
            }
            self.visible = false;
            self.active_pose = None;
            return None;
        }

        // Placement is captured only on the hidden -> visible edge. This keeps the panel
        // world-locked instead of accidentally turning an "in front" spawn into a head-locked HUD.
        // The panel is furniture, not a face-hugger.
        if self.visible {
            return self.active_pose;
        }

        let pose = match target.spawn_mode {
            OverlaySpawnMode::RestoreLastPosition => self
                .saved_pose
                .and_then(sanitize_pose)
                .or(target.remembered_pose.and_then(sanitize_pose))
                .or_else(|| {
                    // No old parking spot? Fine. Materialize politely in front of the headset instead.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn target(spawn_mode: OverlaySpawnMode, remembered_pose: Option<PanelPose>) -> SurfaceTarget {
        SurfaceTarget {
            visible: true,
            distance_m: 1.0,
            spawn_mode,
            remembered_pose,
        }
    }

    fn pose(x: f32) -> PanelPose {
        PanelPose {
            position: [x, 1.5, -1.0],
            orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
        }
    }

    #[test]
    fn persisted_pose_seeds_restore_after_fresh_start() {
        let mut controller = SurfaceController::default();
        let persisted = pose(2.0);
        let restored = controller.sync(
            target(OverlaySpawnMode::RestoreLastPosition, Some(persisted)),
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        );
        assert_eq!(restored, Some(persisted));
        assert_eq!(controller.saved_pose(), Some(persisted));
    }

    #[test]
    fn current_process_pose_wins_over_stale_persisted_pose() {
        let mut controller = SurfaceController::default();
        let current = pose(1.0);
        assert!(controller.remember_pose(current));
        let restored = controller.sync(
            target(OverlaySpawnMode::RestoreLastPosition, Some(pose(9.0))),
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        );
        assert_eq!(restored, Some(current));
    }

    #[test]
    fn live_move_does_not_replace_saved_pose_until_commit() {
        let mut controller = SurfaceController::default();
        let original = pose(1.0);
        assert!(controller.remember_pose(original));
        let _ = controller.sync(
            target(OverlaySpawnMode::RestoreLastPosition, None),
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        );
        let moved = pose(2.0);
        assert!(controller.move_active_pose(moved));
        assert_eq!(controller.active_pose(), Some(moved));
        assert_eq!(controller.saved_pose(), Some(original));
        assert!(controller.remember_pose(moved));
        assert_eq!(controller.saved_pose(), Some(moved));
    }

    #[test]
    fn invalid_persisted_pose_falls_back_to_in_front() {
        let state = OverlayState {
            visible: true,
            spawn_mode: OverlaySpawnMode::RestoreLastPosition,
            remembered_pose: Some(OverlayPose {
                position: [f32::NAN, 0.0, 0.0],
                orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
            }),
            ..OverlayState::default()
        };
        let mut controller = SurfaceController::default();
        let restored = controller.sync(
            SurfaceTarget::from(&state),
            [0.0, 1.6, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        );
        assert!(restored.is_some());
        assert_ne!(restored.unwrap().position[0].is_nan(), true);
    }
}
