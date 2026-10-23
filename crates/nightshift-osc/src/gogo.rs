use crate::avatar_context::AvatarContext;
use nightshift_gogo::{GogoCommand, GogoValue, foot_lock_command, looks_like_gogo_1_8};
use nightshift_osc_script::{CURRENT_SCRIPT_VERSION, OscAction, OscScript, OscValue};

pub fn supports_gogo_1_8(context: &AvatarContext) -> bool {
    looks_like_gogo_1_8(context.parameters.iter().map(|parameter| parameter.address.as_str()))
}

pub fn sleep_transition_script(context: &AvatarContext, sleeping: bool) -> Option<OscScript> {
    supports_gogo_1_8(context).then(|| OscScript {
        version: CURRENT_SCRIPT_VERSION,
        actions: vec![command_to_action(foot_lock_command(sleeping))],
    })
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
    use nightshift_gogo::{GOGO_EMOTE, GOGO_FLOAT, GOGO_STATIONARY};

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
}
