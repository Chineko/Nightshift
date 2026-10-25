use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use tokio::sync::{Mutex, OwnedMutexGuard};

const RELATIONSHIP_READ_INTERVAL: Duration = Duration::from_millis(250);
const INVITE_WRITE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestClass {
    RelationshipRead,
    InviteWrite,
}

impl RequestClass {
    fn minimum_interval(self, intervals: RequestIntervals) -> Duration {
        match self {
            Self::RelationshipRead => intervals.relationship_read,
            Self::InviteWrite => intervals.invite_write,
        }
    }
}

#[derive(Clone, Copy)]
struct RequestIntervals {
    relationship_read: Duration,
    invite_write: Duration,
}

impl Default for RequestIntervals {
    fn default() -> Self {
        Self {
            relationship_read: RELATIONSHIP_READ_INTERVAL,
            invite_write: INVITE_WRITE_INTERVAL,
        }
    }
}

struct Schedule {
    next_relationship_read: Instant,
    next_invite_write: Instant,
}

impl Schedule {
    fn new(now: Instant) -> Self {
        Self {
            next_relationship_read: now,
            next_invite_write: now,
        }
    }

    fn next_allowed(&self, class: RequestClass) -> Instant {
        match class {
            RequestClass::RelationshipRead => self.next_relationship_read,
            RequestClass::InviteWrite => self.next_invite_write,
        }
    }

    fn reserve(&mut self, class: RequestClass, now: Instant, interval: Duration) {
        let next = now + interval;
        match class {
            RequestClass::RelationshipRead => self.next_relationship_read = next,
            RequestClass::InviteWrite => self.next_invite_write = next,
        }
    }
}

#[derive(Clone)]
pub struct RequestCoordinator {
    schedule: Arc<Mutex<Schedule>>,
    intervals: RequestIntervals,
}

impl Default for RequestCoordinator {
    fn default() -> Self {
        Self::new(RequestIntervals::default())
    }
}

impl RequestCoordinator {
    fn new(intervals: RequestIntervals) -> Self {
        Self {
            schedule: Arc::new(Mutex::new(Schedule::new(Instant::now()))),
            intervals,
        }
    }

    pub async fn acquire(&self, class: RequestClass) -> RequestPermit {
        // One clipboard at a time. The goblins have been informed.
        let mut guard = self.schedule.clone().lock_owned().await;
        let now = Instant::now();
        let next = guard.next_allowed(class);
        if next > now {
            tokio::time::sleep(next.duration_since(now)).await;
        }

        let now = Instant::now();
        guard.reserve(class, now, class.minimum_interval(self.intervals));
        RequestPermit { _guard: guard }
    }
}

pub struct RequestPermit {
    _guard: OwnedMutexGuard<Schedule>,
}

pub fn shared_vrchat_requests() -> &'static RequestCoordinator {
    static COORDINATOR: OnceLock<RequestCoordinator> = OnceLock::new();
    COORDINATOR.get_or_init(RequestCoordinator::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_coordinator() -> RequestCoordinator {
        RequestCoordinator::new(RequestIntervals {
            relationship_read: Duration::from_millis(5),
            invite_write: Duration::from_millis(15),
        })
    }

    #[test]
    fn request_classes_have_separate_spacing() {
        let intervals = RequestIntervals::default();
        assert_eq!(
            RequestClass::RelationshipRead.minimum_interval(intervals),
            RELATIONSHIP_READ_INTERVAL
        );
        assert_eq!(
            RequestClass::InviteWrite.minimum_interval(intervals),
            INVITE_WRITE_INTERVAL
        );
    }

    #[tokio::test]
    async fn permit_serializes_parallel_requests() {
        let coordinator = fast_coordinator();
        let first = coordinator.acquire(RequestClass::RelationshipRead).await;

        let second_coordinator = coordinator.clone();
        let second = tokio::spawn(async move {
            second_coordinator
                .acquire(RequestClass::RelationshipRead)
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!second.is_finished());

        drop(first);
        let _second = second.await.unwrap();
    }

    #[tokio::test]
    async fn invite_writes_keep_their_minimum_gap() {
        let coordinator = fast_coordinator();
        let first = coordinator.acquire(RequestClass::InviteWrite).await;
        drop(first);

        let started = Instant::now();
        let second = coordinator.acquire(RequestClass::InviteWrite).await;
        assert!(started.elapsed() >= Duration::from_millis(10));
        drop(second);
    }
}
