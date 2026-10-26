use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use tokio::sync::{Mutex, OwnedMutexGuard};

const GLOBAL_REQUEST_INTERVAL: Duration = Duration::from_secs(4);
const SESSION_READ_INTERVAL: Duration = Duration::from_millis(250);
const RELATIONSHIP_READ_INTERVAL: Duration = Duration::from_millis(250);
const STATUS_WRITE_INTERVAL: Duration = Duration::from_secs(10);
const INVITE_WRITE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestClass {
    SessionRead,
    RelationshipRead,
    StatusWrite,
    InviteWrite,
}

impl RequestClass {
    fn minimum_interval(self, intervals: RequestIntervals) -> Duration {
        match self {
            Self::SessionRead => intervals.session_read,
            Self::RelationshipRead => intervals.relationship_read,
            Self::StatusWrite => intervals.status_write,
            Self::InviteWrite => intervals.invite_write,
        }
    }
}

#[derive(Clone, Copy)]
struct RequestIntervals {
    global: Duration,
    session_read: Duration,
    relationship_read: Duration,
    status_write: Duration,
    invite_write: Duration,
}

impl Default for RequestIntervals {
    fn default() -> Self {
        Self {
            global: GLOBAL_REQUEST_INTERVAL,
            session_read: SESSION_READ_INTERVAL,
            relationship_read: RELATIONSHIP_READ_INTERVAL,
            status_write: STATUS_WRITE_INTERVAL,
            invite_write: INVITE_WRITE_INTERVAL,
        }
    }
}

#[derive(Clone)]
struct ClassGate {
    next_allowed: Arc<Mutex<Instant>>,
}

impl ClassGate {
    fn new(now: Instant) -> Self {
        Self {
            next_allowed: Arc::new(Mutex::new(now)),
        }
    }

    async fn acquire(&self, interval: Duration) -> OwnedMutexGuard<Instant> {
        let mut guard = self.next_allowed.clone().lock_owned().await;
        let now = Instant::now();
        if *guard > now {
            tokio::time::sleep(guard.duration_since(now)).await;
        }

        *guard = Instant::now() + interval;
        guard
    }
}

#[derive(Clone)]
pub struct RequestCoordinator {
    global: ClassGate,
    session_read: ClassGate,
    relationship_read: ClassGate,
    status_write: ClassGate,
    invite_write: ClassGate,
    intervals: RequestIntervals,
}

impl Default for RequestCoordinator {
    fn default() -> Self {
        Self::new(RequestIntervals::default())
    }
}

impl RequestCoordinator {
    fn new(intervals: RequestIntervals) -> Self {
        let now = Instant::now();
        Self {
            global: ClassGate::new(now),
            session_read: ClassGate::new(now),
            relationship_read: ClassGate::new(now),
            status_write: ClassGate::new(now),
            invite_write: ClassGate::new(now),
            intervals,
        }
    }

    pub async fn acquire(&self, class: RequestClass) -> RequestPermit {
        // Keep each request class orderly, then pass every request through one shared gate.
        // OyasumiVR caps all VRChat API traffic at 15 requests/minute; four seconds between
        // request starts gives Nightshift the same global budget while preserving the stricter
        // per-class spacing below. Hydra gets one queue, not five goblins at the same door.
        let class_guard = match class {
            RequestClass::SessionRead => {
                self.session_read
                    .acquire(class.minimum_interval(self.intervals))
                    .await
            }
            RequestClass::RelationshipRead => {
                self.relationship_read
                    .acquire(class.minimum_interval(self.intervals))
                    .await
            }
            RequestClass::StatusWrite => {
                self.status_write
                    .acquire(class.minimum_interval(self.intervals))
                    .await
            }
            RequestClass::InviteWrite => {
                self.invite_write
                    .acquire(class.minimum_interval(self.intervals))
                    .await
            }
        };
        let global_guard = self.global.acquire(self.intervals.global).await;

        RequestPermit {
            _class_guard: class_guard,
            _global_guard: global_guard,
        }
    }
}

pub struct RequestPermit {
    _class_guard: OwnedMutexGuard<Instant>,
    _global_guard: OwnedMutexGuard<Instant>,
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
            global: Duration::from_millis(9),
            session_read: Duration::from_millis(5),
            relationship_read: Duration::from_millis(7),
            status_write: Duration::from_millis(15),
            invite_write: Duration::from_millis(11),
        })
    }

    #[test]
    fn request_classes_have_separate_spacing() {
        let intervals = RequestIntervals::default();
        assert_eq!(intervals.global, GLOBAL_REQUEST_INTERVAL);
        assert_eq!(
            RequestClass::SessionRead.minimum_interval(intervals),
            SESSION_READ_INTERVAL
        );
        assert_eq!(
            RequestClass::RelationshipRead.minimum_interval(intervals),
            RELATIONSHIP_READ_INTERVAL
        );
        assert_eq!(
            RequestClass::StatusWrite.minimum_interval(intervals),
            STATUS_WRITE_INTERVAL
        );
        assert_eq!(
            RequestClass::InviteWrite.minimum_interval(intervals),
            INVITE_WRITE_INTERVAL
        );
    }

    #[tokio::test]
    async fn permit_serializes_parallel_requests_within_one_class() {
        let coordinator = fast_coordinator();
        let first = coordinator.acquire(RequestClass::SessionRead).await;

        let second_coordinator = coordinator.clone();
        let second = tokio::spawn(async move {
            second_coordinator.acquire(RequestClass::SessionRead).await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!second.is_finished());

        drop(first);
        let _second = second.await.unwrap();
    }

    #[tokio::test]
    async fn status_writes_keep_their_minimum_gap() {
        let coordinator = fast_coordinator();
        let first = coordinator.acquire(RequestClass::StatusWrite).await;
        drop(first);

        let started = Instant::now();
        let second = coordinator.acquire(RequestClass::StatusWrite).await;
        assert!(started.elapsed() >= Duration::from_millis(10));
        drop(second);
    }

    #[tokio::test]
    async fn global_budget_spaces_independent_request_classes() {
        let coordinator = fast_coordinator();
        let first = coordinator.acquire(RequestClass::SessionRead).await;
        drop(first);

        let started = Instant::now();
        let second = coordinator.acquire(RequestClass::RelationshipRead).await;
        assert!(started.elapsed() >= Duration::from_millis(7));
        drop(second);
    }

    #[tokio::test]
    async fn class_permit_stays_exclusive_while_waiting_for_global_budget() {
        let coordinator = fast_coordinator();
        let global_slot = coordinator.acquire(RequestClass::RelationshipRead).await;
        drop(global_slot);

        let first_coordinator = coordinator.clone();
        let first = tokio::spawn(async move {
            first_coordinator.acquire(RequestClass::SessionRead).await
        });
        tokio::time::sleep(Duration::from_millis(2)).await;

        let second_coordinator = coordinator.clone();
        let second = tokio::spawn(async move {
            second_coordinator.acquire(RequestClass::SessionRead).await
        });
        tokio::time::sleep(Duration::from_millis(4)).await;
        assert!(!second.is_finished());

        let first = first.await.unwrap();
        drop(first);
        let _second = second.await.unwrap();
    }
}
