use super::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// In-memory [`CounterStore`] that records how often it was touched, so tests
/// can prove whether a decision required a write.
#[derive(Default)]
struct MockStore {
    counters: RefCell<HashMap<String, WindowCounter>>,
    loads: Cell<usize>,
    saves: Cell<usize>,
}

impl MockStore {
    fn loads(&self) -> usize {
        self.loads.get()
    }

    fn saves(&self) -> usize {
        self.saves.get()
    }

    fn counter(&self, key: &str) -> Option<WindowCounter> {
        self.counters.borrow().get(key).copied()
    }
}

#[async_trait(?Send)]
impl CounterStore for MockStore {
    async fn load(&self, key: &str) -> Option<WindowCounter> {
        self.loads.set(self.loads.get() + 1);
        self.counters.borrow().get(key).copied()
    }

    async fn save(&self, key: &str, counter: &WindowCounter) {
        self.saves.set(self.saves.get() + 1);
        self.counters.borrow_mut().insert(key.to_string(), *counter);
    }
}

fn admit_blocking(store: &MockStore, key: &str, now: u64, limit: u32) -> Admission {
    futures::executor::block_on(admit(store, key, now, limit))
}

#[test]
fn test_admits_up_to_the_limit() {
    let store = MockStore::default();
    let key = key_for("tag");

    for expected in 1..=3 {
        assert_eq!(admit_blocking(&store, &key, 1_000, 3), Admission::Admitted);
        assert_eq!(store.counter(&key).unwrap().count, expected);
    }
    assert_eq!(store.saves(), 3);
}

#[test]
fn test_rejects_the_request_over_the_limit() {
    let store = MockStore::default();
    let key = key_for("tag");

    for _ in 0..3 {
        admit_blocking(&store, &key, 1_000, 3);
    }

    assert_eq!(
        admit_blocking(&store, &key, 1_010, 3),
        Admission::Limited { retry_after: 50 }
    );
}

#[test]
fn test_rejection_does_not_write() {
    let store = MockStore::default();
    let key = key_for("tag");

    for _ in 0..2 {
        admit_blocking(&store, &key, 1_000, 2);
    }
    assert_eq!(
        admit_blocking(&store, &key, 1_000, 2),
        Admission::Limited { retry_after: 60 }
    );

    assert_eq!(store.saves(), 2, "a rejected request must not be persisted");
    assert_eq!(store.counter(&key).unwrap().count, 2);
}

#[test]
fn test_retry_after_shrinks_as_the_window_advances() {
    let store = MockStore::default();
    let key = key_for("tag");
    admit_blocking(&store, &key, 1_000, 1);

    assert_eq!(
        admit_blocking(&store, &key, 1_030, 1),
        Admission::Limited { retry_after: 30 }
    );
    assert_eq!(
        admit_blocking(&store, &key, 1_059, 1),
        Admission::Limited { retry_after: 1 }
    );
}

#[test]
fn test_window_rolls_after_a_minute() {
    let store = MockStore::default();
    let key = key_for("tag");
    admit_blocking(&store, &key, 1_000, 1);
    assert!(matches!(
        admit_blocking(&store, &key, 1_059, 1),
        Admission::Limited { .. }
    ));

    assert_eq!(admit_blocking(&store, &key, 1_060, 1), Admission::Admitted);
    let counter = store.counter(&key).unwrap();
    assert_eq!(counter.window_start, 1_060);
    assert_eq!(counter.count, 1);
}

#[test]
fn test_window_is_anchored_at_the_first_request() {
    let store = MockStore::default();
    let key = key_for("tag");

    admit_blocking(&store, &key, 1_000, 2);
    admit_blocking(&store, &key, 1_045, 2);

    let counter = store.counter(&key).unwrap();
    assert_eq!(counter.window_start, 1_000);
    assert_eq!(counter.count, 2);
}

#[test]
fn test_clock_going_backwards_does_not_reset_the_window() {
    let store = MockStore::default();
    let key = key_for("tag");
    admit_blocking(&store, &key, 2_000, 1);

    // `now` below `window_start` saturates to zero elapsed seconds, so the
    // window stays open and the source stays limited.
    assert_eq!(
        admit_blocking(&store, &key, 1_000, 1),
        Admission::Limited { retry_after: 60 }
    );
}

#[test]
fn test_disabled_limit_touches_no_storage() {
    let store = MockStore::default();
    let key = key_for("tag");

    for _ in 0..10 {
        assert_eq!(admit_blocking(&store, &key, 1_000, 0), Admission::Admitted);
    }

    assert_eq!(store.loads(), 0);
    assert_eq!(store.saves(), 0);
    assert!(store.counter(&key).is_none());
}

#[test]
fn test_sources_have_independent_counters() {
    let store = MockStore::default();
    let first = key_for("aaaa");
    let second = key_for("bbbb");

    for _ in 0..2 {
        admit_blocking(&store, &first, 1_000, 2);
    }
    assert!(matches!(
        admit_blocking(&store, &first, 1_000, 2),
        Admission::Limited { .. }
    ));
    assert_eq!(
        admit_blocking(&store, &second, 1_000, 2),
        Admission::Admitted
    );
}

#[test]
fn test_missing_counter_starts_at_the_current_time() {
    let store = MockStore::default();
    let key = key_for("tag");

    assert_eq!(admit_blocking(&store, &key, 4_242, 5), Admission::Admitted);
    let counter = store.counter(&key).unwrap();
    assert_eq!(counter.window_start, 4_242);
    assert_eq!(counter.count, 1);
}

#[test]
fn test_is_stale_needs_two_windows() {
    let counter = WindowCounter::new(1_000);
    assert!(!counter.is_stale(1_119));
    assert!(counter.is_stale(1_120));
    assert!(counter.is_stale(9_999));
}

#[test]
fn test_is_stale_tolerates_clock_going_backwards() {
    let counter = WindowCounter::new(1_000);
    assert!(!counter.is_stale(999));
}

#[test]
fn test_retry_after_is_never_zero() {
    let counter = WindowCounter::new(1_000);
    assert_eq!(counter.retry_after(1_000), 60);
    assert_eq!(counter.retry_after(1_059), 1);
    assert_eq!(counter.retry_after(1_060), 1);
    assert_eq!(counter.retry_after(2_000), 1);
}

#[test]
fn test_key_for_prefixes_the_tag() {
    assert_eq!(key_for("deadbeef"), "rate:deadbeef");
    assert!(key_for("x").starts_with(RATE_KEY_PREFIX));
}

#[test]
fn test_default_source_key_is_the_tag_key() {
    let store = MockStore::default();
    let key = futures::executor::block_on(store.key_for_source("0a0b0c0d0e0f1011"));
    assert_eq!(key, "rate:0a0b0c0d0e0f1011");
}

#[test]
fn test_overflow_tag_cannot_collide_with_a_source_tag() {
    // Source tags are hex digests, so a tag that is not all hex digits cannot
    // name a real source.
    assert_eq!(key_for(OVERFLOW_TAG), "rate:overflow");
    assert!(!OVERFLOW_TAG.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_decode_round_trips_a_counter() {
    let counter = WindowCounter {
        window_start: 1_700_000_000,
        count: 7,
    };
    let value = serde_json::to_value(counter).unwrap();
    assert_eq!(decode(&value), Some(counter));
}

#[test]
fn test_decode_rejects_garbage() {
    assert_eq!(decode(&serde_json::json!("nope")), None);
    assert_eq!(decode(&serde_json::json!({"window_start": 1})), None);
    assert_eq!(decode(&serde_json::json!({})), None);
}

#[test]
fn test_counter_admits_without_a_store() {
    // The per-connection message budget keeps its counter in memory, so the
    // decision has to live on `WindowCounter` itself.
    let mut counter = WindowCounter::new(1_000);

    assert_eq!(counter.admit(1_000, 2), Admission::Admitted);
    assert_eq!(counter.admit(1_001, 2), Admission::Admitted);
    assert_eq!(
        counter.admit(1_002, 2),
        Admission::Limited { retry_after: 58 }
    );
    assert_eq!(counter.count, 2, "a refusal must not consume a slot");
}

#[test]
fn test_counter_admits_again_after_the_window_rolls() {
    let mut counter = WindowCounter::new(1_000);
    assert_eq!(counter.admit(1_000, 1), Admission::Admitted);
    assert_eq!(
        counter.admit(1_059, 1),
        Admission::Limited { retry_after: 1 }
    );
    assert_eq!(counter.admit(1_060, 1), Admission::Admitted);
    assert_eq!(counter.window_start, 1_060);
    assert_eq!(counter.count, 1);
}

#[test]
fn test_counter_with_no_limit_never_counts() {
    let mut counter = WindowCounter::new(1_000);
    for _ in 0..10 {
        assert_eq!(counter.admit(1_000, 0), Admission::Admitted);
    }
    assert_eq!(counter.count, 0);
}
