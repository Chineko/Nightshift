use std::collections::{HashSet, VecDeque};

pub const RECENT_INVITE_CAPACITY: usize = 256;

#[derive(Debug)]
pub struct RecentInviteNotifications {
    order: VecDeque<String>,
    ids: HashSet<String>,
    capacity: usize,
}

impl Default for RecentInviteNotifications {
    fn default() -> Self {
        Self::new(RECENT_INVITE_CAPACITY)
    }
}

impl RecentInviteNotifications {
    pub fn new(capacity: usize) -> Self {
        Self {
            order: VecDeque::new(),
            ids: HashSet::new(),
            capacity,
        }
    }

    /// Returns true only the first time an id is observed while it remains in the bounded window.
    pub fn observe(&mut self, id: &str) -> bool {
        if self.capacity == 0 || self.ids.contains(id) {
            return false;
        }

        let id = id.to_owned();
        self.ids.insert(id.clone());
        self.order.push_back(id);

        while self.order.len() > self.capacity {
            if let Some(expired) = self.order.pop_front() {
                self.ids.remove(&expired);
            }
        }
        true
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.ids.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_is_rejected_inside_window() {
        let mut recent = RecentInviteNotifications::new(4);
        assert!(recent.observe("not_1"));
        assert!(!recent.observe("not_1"));
    }

    #[test]
    fn oldest_id_becomes_eligible_after_eviction() {
        let mut recent = RecentInviteNotifications::new(2);
        assert!(recent.observe("not_1"));
        assert!(recent.observe("not_2"));
        assert!(recent.observe("not_3"));
        assert!(recent.observe("not_1"));
    }

    #[test]
    fn clear_resets_window_for_new_session() {
        let mut recent = RecentInviteNotifications::default();
        assert!(recent.observe("not_1"));
        recent.clear();
        assert!(recent.observe("not_1"));
    }

    #[test]
    fn zero_capacity_disables_acceptance() {
        let mut recent = RecentInviteNotifications::new(0);
        assert!(!recent.observe("not_1"));
    }
}
