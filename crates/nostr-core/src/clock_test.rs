use super::*;

#[test]
fn test_now_is_after_2023() {
    // 1_700_000_000 is 2023-11-14; a clock that reads earlier is not the wall
    // clock (for example a zero from a failed conversion).
    assert!(now() > 1_700_000_000);
}
