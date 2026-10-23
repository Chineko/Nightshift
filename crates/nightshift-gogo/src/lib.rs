#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GogoValue {
    Bool(bool),
    Int(i32),
    Float(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GogoCommand {
    pub address: &'static str,
    pub value: GogoValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepingPose {
    Back,
    Front,
    Left,
    Right,
}

pub const GOGO_FLOAT: &str = "/avatar/parameters/Go/Float";
pub const GOGO_STATIONARY: &str = "/avatar/parameters/Go/Stationary";
pub const GOGO_EMOTE: &str = "/avatar/parameters/Go/VRCEmote";
pub const VRC_EMOTE: &str = "/avatar/parameters/VRCEmote";

pub fn pose_value(pose: SleepingPose) -> f32 {
    match pose {
        SleepingPose::Back => -1.0,
        SleepingPose::Front => -0.75,
        SleepingPose::Left => -0.6,
        SleepingPose::Right => -0.4,
    }
}

pub fn pose_commands(pose: SleepingPose) -> [GogoCommand; 3] {
    [
        GogoCommand {
            address: VRC_EMOTE,
            value: GogoValue::Int(214),
        },
        GogoCommand {
            address: GOGO_EMOTE,
            value: GogoValue::Int(214),
        },
        GogoCommand {
            address: GOGO_FLOAT,
            value: GogoValue::Float(pose_value(pose)),
        },
    ]
}

pub fn foot_lock_command(locked: bool) -> GogoCommand {
    GogoCommand {
        address: GOGO_STATIONARY,
        value: GogoValue::Bool(locked),
    }
}

pub fn looks_like_gogo_1_8<'a, I>(addresses: I) -> bool
where
    I: IntoIterator<Item = &'a str>,
{
    let mut has_float = false;
    let mut has_stationary = false;
    let mut has_emote = false;

    for address in addresses {
        match address {
            GOGO_FLOAT => has_float = true,
            GOGO_STATIONARY => has_stationary = true,
            GOGO_EMOTE | VRC_EMOTE => has_emote = true,
            _ => {}
        }
    }

    has_float && has_stationary && has_emote
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gogo_1_8_pose_values_match_proven_preset() {
        assert_eq!(pose_value(SleepingPose::Back), -1.0);
        assert_eq!(pose_value(SleepingPose::Front), -0.75);
        assert_eq!(pose_value(SleepingPose::Left), -0.6);
        assert_eq!(pose_value(SleepingPose::Right), -0.4);
    }

    #[test]
    fn foot_lock_uses_stationary_boolean() {
        assert_eq!(
            foot_lock_command(true),
            GogoCommand {
                address: GOGO_STATIONARY,
                value: GogoValue::Bool(true),
            }
        );
        assert_eq!(
            foot_lock_command(false),
            GogoCommand {
                address: GOGO_STATIONARY,
                value: GogoValue::Bool(false),
            }
        );
    }

    #[test]
    fn detects_minimum_gogo_1_8_parameter_set() {
        assert!(looks_like_gogo_1_8([
            GOGO_FLOAT,
            GOGO_STATIONARY,
            GOGO_EMOTE,
        ]));
        assert!(looks_like_gogo_1_8([
            GOGO_FLOAT,
            GOGO_STATIONARY,
            VRC_EMOTE,
        ]));
        assert!(!looks_like_gogo_1_8([GOGO_FLOAT, GOGO_STATIONARY]));
    }
}
