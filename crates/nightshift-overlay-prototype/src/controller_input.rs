use std::sync::atomic::{AtomicU8, Ordering};

use anyhow::{Context, Result};
use openxr as xr;

use crate::{
    interaction::PanelRay,
    placement::{PanelPose, normalize_quaternion_xyzw},
};

const LEFT_HAND_BIT: u8 = 1;
const RIGHT_HAND_BIT: u8 = 2;

static TRIGGER_DOWN_MASK: AtomicU8 = AtomicU8::new(0);
static TRIGGER_JUST_PRESSED_MASK: AtomicU8 = AtomicU8::new(0);
static SLIDER_DRAG_HAND: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerHand {
    Left,
    Right,
}

fn hand_bit(hand: ControllerHand) -> u8 {
    match hand {
        ControllerHand::Left => LEFT_HAND_BIT,
        ControllerHand::Right => RIGHT_HAND_BIT,
    }
}

pub fn begin_slider_drag_for_pressed_hand() -> bool {
    let pressed = TRIGGER_JUST_PRESSED_MASK.load(Ordering::Relaxed);
    let hand = if pressed & LEFT_HAND_BIT != 0 {
        LEFT_HAND_BIT
    } else if pressed & RIGHT_HAND_BIT != 0 {
        RIGHT_HAND_BIT
    } else {
        0
    };
    if hand == 0 {
        return false;
    }
    SLIDER_DRAG_HAND.store(hand, Ordering::Relaxed);
    true
}

pub fn slider_drag_active() -> bool {
    SLIDER_DRAG_HAND.load(Ordering::Relaxed) != 0
}

fn slider_drag_owned_by(hand: ControllerHand) -> bool {
    SLIDER_DRAG_HAND.load(Ordering::Relaxed) == hand_bit(hand)
}

fn finish_trigger_sample_batch() {
    let drag_hand = SLIDER_DRAG_HAND.load(Ordering::Relaxed);
    if drag_hand != 0 && TRIGGER_DOWN_MASK.load(Ordering::Relaxed) & drag_hand == 0 {
        SLIDER_DRAG_HAND.store(0, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerSample {
    pub hand: ControllerHand,
    pub ray: Option<PanelRay>,
    pub controller_pose: Option<PanelPose>,
    pub trigger_just_pressed: bool,
    pub trigger_down: bool,
    pub grip_down: bool,
    pub grip_just_pressed: bool,
    pub grip_just_released: bool,
}

pub fn select_active_pointer_hand(
    samples: &[PointerSample; 2],
    hits: [bool; 2],
    previous: Option<ControllerHand>,
) -> Option<ControllerHand> {
    for (index, sample) in samples.iter().enumerate() {
        if hits[index] && (sample.trigger_just_pressed || sample.grip_just_pressed) {
            return Some(sample.hand);
        }
    }

    if let Some(previous) = previous
        && samples
            .iter()
            .enumerate()
            .any(|(index, sample)| hits[index] && sample.hand == previous)
    {
        return Some(previous);
    }

    samples
        .iter()
        .enumerate()
        .find_map(|(index, sample)| hits[index].then_some(sample.hand))
}

pub struct ControllerInput {
    action_set: xr::ActionSet,
    left_aim: xr::Action<xr::Posef>,
    right_aim: xr::Action<xr::Posef>,
    left_grip_pose: xr::Action<xr::Posef>,
    right_grip_pose: xr::Action<xr::Posef>,
    left_trigger: xr::Action<bool>,
    right_trigger: xr::Action<bool>,
    left_grab: xr::Action<bool>,
    right_grab: xr::Action<bool>,
    left_aim_space: xr::Space,
    right_aim_space: xr::Space,
    left_grip_space: xr::Space,
    right_grip_space: xr::Space,
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
        let left_grip_pose = action_set
            .create_action::<xr::Posef>("left_grip_pose", "Left grip pose", &[])
            .context("creating left grip pose action")?;
        let right_grip_pose = action_set
            .create_action::<xr::Posef>("right_grip_pose", "Right grip pose", &[])
            .context("creating right grip pose action")?;
        let left_trigger = action_set
            .create_action::<bool>("left_trigger", "Left UI trigger", &[])
            .context("creating left trigger action")?;
        let right_trigger = action_set
            .create_action::<bool>("right_trigger", "Right UI trigger", &[])
            .context("creating right trigger action")?;
        let left_grab = action_set
            .create_action::<bool>("left_grab", "Left panel grab", &[])
            .context("creating left grab action")?;
        let right_grab = action_set
            .create_action::<bool>("right_grab", "Right panel grab", &[])
            .context("creating right grab action")?;

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
                        &left_grip_pose,
                        instance.string_to_path("/user/hand/left/input/grip/pose")?,
                    ),
                    xr::Binding::new(
                        &right_grip_pose,
                        instance.string_to_path("/user/hand/right/input/grip/pose")?,
                    ),
                    xr::Binding::new(
                        &left_trigger,
                        instance.string_to_path("/user/hand/left/input/select/click")?,
                    ),
                    xr::Binding::new(
                        &right_trigger,
                        instance.string_to_path("/user/hand/right/input/select/click")?,
                    ),
                ],
            )
            .context("suggesting simple-controller panel bindings")?;

        let touch_profile = instance
            .string_to_path("/interaction_profiles/oculus/touch_controller")
            .context("resolving Oculus Touch interaction profile")?;
        instance
            .suggest_interaction_profile_bindings(
                touch_profile,
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
                        &left_grip_pose,
                        instance.string_to_path("/user/hand/left/input/grip/pose")?,
                    ),
                    xr::Binding::new(
                        &right_grip_pose,
                        instance.string_to_path("/user/hand/right/input/grip/pose")?,
                    ),
                    xr::Binding::new(
                        &left_trigger,
                        instance.string_to_path("/user/hand/left/input/trigger/value")?,
                    ),
                    xr::Binding::new(
                        &right_trigger,
                        instance.string_to_path("/user/hand/right/input/trigger/value")?,
                    ),
                    xr::Binding::new(
                        &left_grab,
                        instance.string_to_path("/user/hand/left/input/squeeze/value")?,
                    ),
                    xr::Binding::new(
                        &right_grab,
                        instance.string_to_path("/user/hand/right/input/squeeze/value")?,
                    ),
                ],
            )
            .context("suggesting Oculus Touch panel bindings")?;

        session
            .attach_action_sets(&[&action_set])
            .context("attaching Nightshift panel action set")?;
        let left_aim_space = left_aim
            .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
            .context("creating left pointer action space")?;
        let right_aim_space = right_aim
            .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
            .context("creating right pointer action space")?;
        let left_grip_space = left_grip_pose
            .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
            .context("creating left grip action space")?;
        let right_grip_space = right_grip_pose
            .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
            .context("creating right grip action space")?;

        Ok(Self {
            action_set,
            left_aim,
            right_aim,
            left_grip_pose,
            right_grip_pose,
            left_trigger,
            right_trigger,
            left_grab,
            right_grab,
            left_aim_space,
            right_aim_space,
            left_grip_space,
            right_grip_space,
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

        TRIGGER_DOWN_MASK.store(0, Ordering::Relaxed);
        TRIGGER_JUST_PRESSED_MASK.store(0, Ordering::Relaxed);

        let samples = [
            sample_hand(
                ControllerHand::Left,
                &self.left_aim,
                &self.left_grip_pose,
                &self.left_trigger,
                &self.left_grab,
                &self.left_aim_space,
                &self.left_grip_space,
                session,
                display_time,
                base_space,
            )?,
            sample_hand(
                ControllerHand::Right,
                &self.right_aim,
                &self.right_grip_pose,
                &self.right_trigger,
                &self.right_grab,
                &self.right_aim_space,
                &self.right_grip_space,
                session,
                display_time,
                base_space,
            )?,
        ];
        finish_trigger_sample_batch();
        Ok(samples)
    }
}

#[allow(clippy::too_many_arguments)]
fn sample_hand(
    hand: ControllerHand,
    aim: &xr::Action<xr::Posef>,
    grip_pose: &xr::Action<xr::Posef>,
    trigger: &xr::Action<bool>,
    grab: &xr::Action<bool>,
    aim_space: &xr::Space,
    grip_space: &xr::Space,
    session: &xr::Session<xr::Vulkan>,
    display_time: xr::Time,
    base_space: &xr::Space,
) -> Result<PointerSample> {
    let trigger_state = trigger
        .state(session, xr::Path::NULL)
        .context("reading controller trigger action")?;
    let grab_state = grab
        .state(session, xr::Path::NULL)
        .context("reading controller grab action")?;
    let trigger_down = trigger_state.is_active && trigger_state.current_state;
    let trigger_edge = trigger_down && trigger_state.changed_since_last_sync;
    let bit = hand_bit(hand);
    if trigger_down {
        TRIGGER_DOWN_MASK.fetch_or(bit, Ordering::Relaxed);
    }
    if trigger_edge {
        TRIGGER_JUST_PRESSED_MASK.fetch_or(bit, Ordering::Relaxed);
    }
    let trigger_just_pressed = trigger_edge || (trigger_down && slider_drag_owned_by(hand));
    let grip_down = grab_state.is_active && grab_state.current_state;
    let grip_just_pressed =
        grab_state.is_active && grab_state.changed_since_last_sync && grab_state.current_state;
    let grip_just_released = grab_state.changed_since_last_sync && !grab_state.current_state;

    let ray = locate_pose_action(aim, aim_space, session, display_time, base_space)?
        .and_then(ray_from_pose);
    let controller_pose =
        locate_pose_action(grip_pose, grip_space, session, display_time, base_space)?
            .and_then(panel_pose_from_xr);

    Ok(PointerSample {
        hand,
        ray,
        controller_pose,
        trigger_just_pressed,
        trigger_down,
        grip_down,
        grip_just_pressed,
        grip_just_released,
    })
}

fn locate_pose_action(
    action: &xr::Action<xr::Posef>,
    space: &xr::Space,
    session: &xr::Session<xr::Vulkan>,
    display_time: xr::Time,
    base_space: &xr::Space,
) -> Result<Option<xr::Posef>> {
    if !action.is_active(session, xr::Path::NULL)? {
        return Ok(None);
    }
    let location = space
        .locate(base_space, display_time)
        .context("locating controller pose")?;
    let valid = location
        .location_flags
        .contains(xr::SpaceLocationFlags::POSITION_VALID)
        && location
            .location_flags
            .contains(xr::SpaceLocationFlags::ORIENTATION_VALID);
    Ok(valid.then_some(location.pose))
}

fn panel_pose_from_xr(pose: xr::Posef) -> Option<PanelPose> {
    Some(PanelPose {
        position: [pose.position.x, pose.position.y, pose.position.z],
        orientation_xyzw: normalize_quaternion_xyzw([
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        ])?,
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

#[cfg(test)]
mod pointer_selection_tests {
    use super::*;

    fn sample(hand: ControllerHand) -> PointerSample {
        PointerSample {
            hand,
            ray: None,
            controller_pose: None,
            trigger_just_pressed: false,
            trigger_down: false,
            grip_down: false,
            grip_just_pressed: false,
            grip_just_released: false,
        }
    }

    #[test]
    fn active_input_wins_over_previous_hand() {
        let left = sample(ControllerHand::Left);
        let mut right = sample(ControllerHand::Right);
        right.trigger_just_pressed = true;
        assert_eq!(
            select_active_pointer_hand(&[left, right], [true, true], Some(ControllerHand::Left)),
            Some(ControllerHand::Right)
        );
    }

    #[test]
    fn previous_hand_is_sticky_while_it_still_hits() {
        let samples = [sample(ControllerHand::Left), sample(ControllerHand::Right)];
        assert_eq!(
            select_active_pointer_hand(&samples, [true, true], Some(ControllerHand::Right)),
            Some(ControllerHand::Right)
        );
    }

    #[test]
    fn selection_falls_back_to_the_only_hit() {
        let samples = [sample(ControllerHand::Left), sample(ControllerHand::Right)];
        assert_eq!(
            select_active_pointer_hand(&samples, [false, true], Some(ControllerHand::Left)),
            Some(ControllerHand::Right)
        );
    }

    #[test]
    fn slider_drag_latches_pressed_hand_until_release() {
        TRIGGER_JUST_PRESSED_MASK.store(LEFT_HAND_BIT, Ordering::Relaxed);
        TRIGGER_DOWN_MASK.store(LEFT_HAND_BIT, Ordering::Relaxed);
        SLIDER_DRAG_HAND.store(0, Ordering::Relaxed);
        assert!(begin_slider_drag_for_pressed_hand());
        assert!(slider_drag_owned_by(ControllerHand::Left));
        finish_trigger_sample_batch();
        assert!(slider_drag_active());

        TRIGGER_DOWN_MASK.store(0, Ordering::Relaxed);
        finish_trigger_sample_batch();
        assert!(!slider_drag_active());
    }
}
