use super::*;

#[test]
fn test_short_no_truncation() {
    assert_eq!(short("hello", 10), "hello");
}

#[test]
fn test_short_exact() {
    assert_eq!(short("hello", 5), "hello");
}

#[test]
fn test_short_truncation() {
    assert_eq!(short("hello world", 5), "hello");
}

#[test]
fn test_short_empty() {
    assert_eq!(short("", 5), "");
}

#[test]
fn test_short_zero_len() {
    assert_eq!(short("a", 0), "");
}

#[test]
fn test_now_returns_nonzero() {
    let t = now();
    assert!(t > 1700000000, "now() should return recent unix timestamp");
}
