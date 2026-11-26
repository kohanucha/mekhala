use super::*;

#[test]
fn test_admits_up_to_the_limit_and_refusal_consumes_no_slot() {
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
fn test_retry_after_shrinks_as_the_window_advances() {
    let mut counter = WindowCounter::new(1_000);
    assert_eq!(counter.admit(1_000, 1), Admission::Admitted);

    assert_eq!(
        counter.admit(1_030, 1),
        Admission::Limited { retry_after: 30 }
    );
    assert_eq!(
        counter.admit(1_059, 1),
        Admission::Limited { retry_after: 1 }
    );
}

#[test]
fn test_window_rolls_after_a_minute() {
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
fn test_window_is_anchored_at_the_first_request() {
    let mut counter = WindowCounter::new(1_000);
    assert_eq!(counter.admit(1_000, 2), Admission::Admitted);
    assert_eq!(counter.admit(1_045, 2), Admission::Admitted);

    assert_eq!(counter.window_start, 1_000);
    assert_eq!(counter.count, 2);
}

#[test]
fn test_clock_going_backwards_does_not_reset_the_window() {
    let mut counter = WindowCounter::new(2_000);
    assert_eq!(counter.admit(2_000, 1), Admission::Admitted);

    // `now` below `window_start` saturates to zero elapsed seconds, so the
    // window stays open and the counter stays limited.
    assert_eq!(
        counter.admit(1_000, 1),
        Admission::Limited { retry_after: 60 }
    );
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
fn test_no_limit_never_counts() {
    let mut counter = WindowCounter::new(1_000);
    for _ in 0..10 {
        assert_eq!(counter.admit(1_000, 0), Admission::Admitted);
    }
    assert_eq!(counter.count, 0);
}
