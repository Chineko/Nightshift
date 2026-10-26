use std::{collections::VecDeque, time::Duration};

use nightshift_protocol::{AppState, InviteActionKind, SleepPhase, StateEvent};

const MAX_TOASTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub title: String,
    pub message: String,
    pub severity: ToastSeverity,
    pub timeout: Duration,
    pub show_while_sleeping: bool,
}

impl Toast {
    pub fn info(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            severity: ToastSeverity::Info,
            timeout: Duration::from_secs(5),
            show_while_sleeping: false,
        }
    }

    pub fn warning(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            severity: ToastSeverity::Warning,
            timeout: Duration::from_secs(7),
            show_while_sleeping: false,
        }
    }

    pub fn error(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            severity: ToastSeverity::Error,
            timeout: Duration::from_secs(9),
            show_while_sleeping: true,
        }
    }
}

pub fn toast_for_event(previous: &AppState, event: &StateEvent) -> Option<Toast> {
    match event {
        StateEvent::SleepChanged(state) if state.phase != previous.sleep.phase => match state.phase {
            SleepPhase::Sleeping => Some(Toast::info(
                "Sleep mode active",
                "Nightshift automation is settling in.",
            )),
            SleepPhase::Awake => Some(Toast::info(
                "Awake",
                "Nightshift restored your awake state.",
            )),
            SleepPhase::EnteringSleep | SleepPhase::Waking => None,
        },
        StateEvent::VrchatChanged(state)
            if previous.vrchat.connected && !state.connected =>
        {
            Some(Toast::warning(
                "VRChat disconnected",
                "Presence and invite automation are paused.",
            ))
        }
        StateEvent::VrchatChanged(state)
            if !previous.vrchat.connected && state.connected =>
        {
            Some(Toast::info(
                "VRChat connected",
                "Presence and invite automation are ready.",
            ))
        }
        StateEvent::InvitesChanged(state)
            if state.last_error != previous.invites.last_error && state.last_error.is_some() =>
        {
            Some(Toast::error(
                "Invite automation",
                state.last_error.as_deref().unwrap_or("Invite handling failed."),
            ))
        }
        StateEvent::InvitesChanged(state)
            if state.last_action != previous.invites.last_action && state.last_action.is_some() =>
        {
            let action = state.last_action.as_ref()?;
            let person = action.display_name.as_deref().unwrap_or("VRChat user");
            let message = match action.kind {
                InviteActionKind::Accepted => format!("Accepted invite from {person}."),
                InviteActionKind::Declined => format!("Declined invite from {person}."),
                InviteActionKind::Ignored => format!("Ignored invite from {person}."),
                InviteActionKind::Asked => format!("Asked before handling invite from {person}."),
            };
            Some(Toast::info("Invite automation", message))
        }
        _ => None,
    }
}

#[derive(Clone, Debug)]
struct ActiveToast {
    toast: Toast,
    age: Duration,
}

#[derive(Clone, Debug, Default)]
pub struct ToastQueue {
    items: VecDeque<ActiveToast>,
}

impl ToastQueue {
    pub fn push(&mut self, toast: Toast, sleeping: bool) {
        if sleeping && !toast.show_while_sleeping {
            return;
        }
        if self.items.len() == MAX_TOASTS {
            self.items.pop_front();
        }
        self.items.push_back(ActiveToast {
            toast,
            age: Duration::ZERO,
        });
    }

    pub fn tick(&mut self, delta: Duration) {
        for item in &mut self.items {
            item.age = item.age.saturating_add(delta);
        }
        self.items.retain(|item| item.age < item.toast.timeout);
    }

    pub fn dismiss(&mut self, index: usize) {
        self.items.remove(index);
    }

    pub fn visible(&self) -> impl Iterator<Item = &Toast> {
        self.items.iter().map(|item| &item.toast)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nightshift_protocol::InviteAction;

    #[test]
    fn queue_is_bounded_and_keeps_newest_toasts() {
        let mut queue = ToastQueue::default();
        for index in 0..=MAX_TOASTS {
            queue.push(Toast::info(format!("toast-{index}"), "message"), false);
        }

        assert_eq!(queue.len(), MAX_TOASTS);
        assert_eq!(queue.visible().next().unwrap().title, "toast-1");
    }

    #[test]
    fn ordinary_toasts_stay_quiet_while_sleeping() {
        let mut queue = ToastQueue::default();
        queue.push(Toast::info("hello", "world"), true);
        assert!(queue.is_empty());
    }

    #[test]
    fn errors_can_break_through_sleep_quiet_mode() {
        let mut queue = ToastQueue::default();
        queue.push(Toast::error("VRChat", "session expired"), true);
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn expired_toasts_leave_the_basement() {
        let mut queue = ToastQueue::default();
        queue.push(Toast::info("temporary", "toast"), false);
        queue.tick(Duration::from_secs(5));
        assert!(queue.is_empty());
    }

    #[test]
    fn dismiss_removes_only_the_selected_toast() {
        let mut queue = ToastQueue::default();
        queue.push(Toast::info("first", "toast"), false);
        queue.push(Toast::warning("second", "toast"), false);
        queue.dismiss(0);

        assert_eq!(queue.len(), 1);
        assert_eq!(queue.visible().next().unwrap().title, "second");
    }

    #[test]
    fn sleep_notifications_only_fire_on_settled_edges() {
        let previous = AppState::default();
        let mut sleeping = previous.sleep.clone();
        sleeping.phase = SleepPhase::Sleeping;

        let toast = toast_for_event(&previous, &StateEvent::SleepChanged(sleeping)).unwrap();
        assert_eq!(toast.title, "Sleep mode active");

        let mut entering = previous.sleep.clone();
        entering.phase = SleepPhase::EnteringSleep;
        assert!(toast_for_event(&previous, &StateEvent::SleepChanged(entering)).is_none());
    }

    #[test]
    fn vrchat_connection_changes_are_notified_without_poll_noise() {
        let mut previous = AppState::default();
        previous.vrchat.connected = true;
        let mut disconnected = previous.vrchat.clone();
        disconnected.connected = false;

        let toast = toast_for_event(&previous, &StateEvent::VrchatChanged(disconnected)).unwrap();
        assert_eq!(toast.severity, ToastSeverity::Warning);
    }

    #[test]
    fn invite_actions_are_notified_only_when_the_action_changes() {
        let previous = AppState::default();
        let mut invites = previous.invites.clone();
        invites.last_action = Some(InviteAction {
            kind: InviteActionKind::Accepted,
            display_name: Some("Sleepy Friend".into()),
        });

        let toast = toast_for_event(&previous, &StateEvent::InvitesChanged(invites)).unwrap();
        assert!(toast.message.contains("Sleepy Friend"));
    }
}
