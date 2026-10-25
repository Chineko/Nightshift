use std::{collections::VecDeque, time::Duration};

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
}
