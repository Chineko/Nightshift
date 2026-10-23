use crate::avatar_context::AvatarContext;
use nightshift_gogo::{
    GogoCommand, GogoValue, SleepingPose as GogoPose, foot_lock_command, looks_like_gogo_1_8,
    pose_commands,
};
use nightshift_osc_script::{CURRENT_SCRIPT_VERSION, OscAction, OscScript, OscValue};
use nightshift_protocol::SleepingPose;

pub const FOOT_LOCK_RELEASE_WINDOW_MS: u64 = 600;

pub fn supports_gogo_1_8(context: &AvatarContext) -> bool {
    looks_like_gogo_1_8(context.parameters.iter().map(|parameter| parameter.address.as_str()))
}

pub fn sleep_transition_script(context: &AvatarContext, sleeping: bool) -> Option<OscScript> {
    supports_gogo_1_8(context).then(|| OscScript {
        version: CURRENT_SCRIPT_VERSION,
        actions: vec![command_to_action(foot_lock_command(sleeping))],
    })
}

pub fn pose_change_script(context: &AvatarContext, pose: SleepingPose) -> Option<OscScript> {
    if !supports_gogo_1_8(context) {
        return None;
    }

    let pose = protocol_to_gogo_pose(pose)?;
    let mut actions = Vec::new();
    actions.push(command_to_action(foot_lock_command(false)));
    actions.extend(pose_commands(pose).into_iter().map(command_to_action));
    actions.push(OscAction::Delay {
        milliseconds: FOOT_LOCK_RELEASE_WINDOW_MS,
    });
    actions.push(command_to_action(foot_lock_command(true)));

    Some(OscScript {
        version: CURRENT_SCRIPT_VERSION,
        actions,
    })
}

fn protocol_to_gogo_pose(pose: SleepingPose) -> Option<GogoPose> {
    match pose {
        SleepingPose::Unknown => None,
        SleepingPose::Front => Some(GogoPose::Front),
        SleepingPose::Back => Some(GogoPose::Back),
        SleepingPose::Left => Some(GogoPose::Left),
        SleepingPose::Right => Some(GogoPose::Right),
    }
}

fn command_to_action(command: GogoCommand) -> OscAction {
    let value = match command.value {
        GogoValue::Bool(value) => OscValue::Bool(value),
        GogoValue::Int(value) => OscValue::Int(value),
        GogoValue::Float(value) => OscValue::Float(value),
    };

    OscAction::Send {
        address: command.address.to_owned(),
        value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::avatar_context::{AvatarParameter, AvatarParameterType};
    use nightshift_gogo::{GOGO_EMOTE, GOGO_FLOAT, GOGO_STATIONARY, VRC_EMOTE};

    fn parameter(address: &str) -> AvatarParameter {
        AvatarParameter {
            value_type: AvatarParameterType::Bool,
            name: address.trim_start_matches("/avatar/parameters/").into(),
            address: address.into(),
            aliases: Vec::new(),
        }
    }

    fn compatible_context() -> AvatarContext {
        AvatarContext {
            avatar_id: "avtr_test".into(),
            parameters: vec![
                parameter(GOGO_FLOAT),
                parameter(GOGO_STATIONARY),
                parameter(GOGO_EMOTE),
            ],
        }
    }

    #[test]
    fn detects_compatible_avatar() {
        assert!(supports_gogo_1_8(&compatible_context()));
    }

    #[test]
    fn refuses_incomplete_avatar() {
        let context = AvatarContext {
            avatar_id: "avtr_test".into(),
            parameters: vec![parameter(GOGO_STATIONARY)],
        };
        assert!(!supports_gogo_1_8(&context));
        assert!(sleep_transition_script(&context, true).is_none());
        assert!(pose_change_script(&context, SleepingPose::Left).is_none());
    }

    #[test]
    fn sleep_locks_and_wake_unlocks_feet() {
        let sleep = sleep_transition_script(&compatible_context(), true).unwrap();
        let wake = sleep_transition_script(&compatible_context(), false).unwrap();

        assert!(matches!(
            &sleep.actions[0],
            OscAction::Send { address, value: OscValue::Bool(true) }
                if address == GOGO_STATIONARY
        ));
        assert!(matches!(
            &wake.actions[0],
            OscAction::Send { address, value: OscValue::Bool(false) }
                if address == GOGO_STATIONARY
        ));
    }

    #[test]
    fn pose_change_unlocks_poses_waits_and_relocks() {
        let script = pose_change_script(&compatible_context(), SleepingPose::Left).unwrap();

        assert!(matches!(
            &script.actions[0],
            OscAction::Send { address, value: OscValue::Bool(false) }
                if address == GOGO_STATIONARY
        ));
        assert!(matches!(
            &script.actions[1],
            OscAction::Send { address, value: OscValue::Int(214) }
                if address == VRC_EMOTE
        ));
        assert!(matches!(
            &script.actions[2],
            OscAction::Send { address, value: OscValue::Int(214) }
                if address == GOGO_EMOTE
        ));
        assert!(matches!(
            &script.actions[3],
            OscAction::Send { address, value: OscValue::Float(value) }
                if address == GOGO_FLOAT && (*value - -0.6).abs() < f32::EPSILON
        ));
        assert_eq!(
            script.actions[4],
            OscAction::Delay {
                milliseconds: FOOT_LOCK_RELEASE_WINDOW_MS,
            }
        );
        assert!(matches!(
            &script.actions[5],
            OscAction::Send { address, value: OscValue::Bool(true) }
                if address == GOGO_STATIONARY
        ));
    }

    #[test]
    fn unknown_pose_does_nothing() {
        assert!(pose_change_script(&compatible_context(), SleepingPose::Unknown).is_none());
    }
}
