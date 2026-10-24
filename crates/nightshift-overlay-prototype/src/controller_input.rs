use anyhow::{Context, Result};
use openxr as xr;

use crate::{interaction::PanelRay, placement::normalize_quaternion_xyzw};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerHand {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerSample {
    pub hand: ControllerHand,
    pub ray: Option<PanelRay>,
    pub select_down: bool,
    pub just_pressed: bool,
    pub just_released: bool,
}

pub struct ControllerInput {
    action_set: xr::ActionSet,
    left_aim: xr::Action<xr::Posef>,
    right_aim: xr::Action<xr::Posef>,
    left_select: xr::Action<bool>,
    right_select: xr::Action<bool>,
    left_space: xr::Space,
    right_space: xr::Space,
}

impl ControllerInput {
    pub fn new(instance: &xr::Instance, session: &xr::Session<xr::Vulkan>) -> Result<Self> {
        let action_set = instance
            .create_action_set("nightshift_panel", "Nightshift panel", 0)
            .context("creating Nightshift panel action set")?;
        let left_aim = action_set
            .create_action::<xr::Posef>("left_aim", "Left pointer", &[])
            .context("creating left pointer pose action")?;
        let right_aim = action_set
            .create_action::<xr::Posef>("right_aim", "Right pointer", &[])
            .context("creating right pointer pose action")?;
        let left_select = action_set
            .create_action::<bool>("left_select", "Left select", &[])
            .context("creating left select action")?;
        let right_select = action_set
            .create_action::<bool>("right_select", "Right select", &[])
            .context("creating right select action")?;

        let simple_profile = instance
            .string_to_path("/interaction_profiles/khr/simple_controller")
            .context("resolving simple-controller interaction profile")?;
        instance
            .suggest_interaction_profile_bindings(
                simple_profile,
                &[
                    xr::Binding::new(
                        &left_aim,
                        instance.string_to_path("/user/hand/left/input/aim/pose")?,
                    ),
                    xr::Binding::new(
                        &right_aim,
                        instance.string_to_path("/user/hand/right/input/aim/pose")?,
                    ),
                    xr::Binding::new(
                        &left_select,
                        instance.string_to_path("/user/hand/left/input/select/click")?,
                    ),
                    xr::Binding::new(
                        &right_select,
                        instance.string_to_path("/user/hand/right/input/select/click")?,
                    ),
                ],
            )
            .context("suggesting simple-controller panel bindings")?;

        session
            .attach_action_sets(&[&action_set])
            .context("attaching Nightshift panel action set")?;
        let left_space = left_aim
            .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
            .context("creating left pointer action space")?;
        let right_space = right_aim
            .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
            .context("creating right pointer action space")?;

        Ok(Self {
            action_set,
            left_aim,
            right_aim,
            left_select,
            right_select,
            left_space,
            right_space,
        })
    }

    pub fn samples(
        &self,
        session: &xr::Session<xr::Vulkan>,
        display_time: xr::Time,
        base_space: &xr::Space,
    ) -> Result<[PointerSample; 2]> {
        session
            .sync_actions(&[(&self.action_set).into()])
            .context("synchronizing Nightshift panel actions")?;

        Ok([
            sample_hand(
                ControllerHand::Left,
                &self.left_aim,
                &self.left_select,
                &self.left_space,
                session,
                display_time,
                base_space,
            )?,
            sample_hand(
                ControllerHand::Right,
                &self.right_aim,
                &self.right_select,
                &self.right_space,
                session,
                display_time,
                base_space,
            )?,
        ])
    }
}

fn sample_hand(
    hand: ControllerHand,
    aim: &xr::Action<xr::Posef>,
    select: &xr::Action<bool>,
    space: &xr::Space,
    session: &xr::Session<xr::Vulkan>,
    display_time: xr::Time,
    base_space: &xr::Space,
) -> Result<PointerSample> {
    let select_state = select
        .state(session, xr::Path::NULL)
        .context("reading controller select action")?;
    let select_down = select_state.is_active && select_state.current_state;
    let just_pressed = select_state.changed_since_last_sync && select_state.current_state;
    let just_released = select_state.changed_since_last_sync && !select_state.current_state;

    let ray = if aim.is_active(session, xr::Path::NULL)? {
        let location = space
            .locate(base_space, display_time)
            .context("locating controller pointer")?;
        let valid = location
            .location_flags
            .contains(xr::SpaceLocationFlags::POSITION_VALID)
            && location
                .location_flags
                .contains(xr::SpaceLocationFlags::ORIENTATION_VALID);
        valid.then(|| ray_from_pose(location.pose)).flatten()
    } else {
        None
    };

    Ok(PointerSample {
        hand,
        ray,
        select_down,
        just_pressed,
        just_released,
    })
}

fn ray_from_pose(pose: xr::Posef) -> Option<PanelRay> {
    let orientation = normalize_quaternion_xyzw([
        pose.orientation.x,
        pose.orientation.y,
        pose.orientation.z,
        pose.orientation.w,
    ])?;
    let direction = rotate([0.0, 0.0, -1.0], orientation);
    Some(PanelRay {
        origin: [pose.position.x, pose.position.y, pose.position.z],
        direction,
    })
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
