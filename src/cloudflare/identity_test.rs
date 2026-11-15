use super::*;

#[test]
fn test_tag_is_fixed_length() {
    assert_eq!(tag("203.0.113.9").len(), TAG_BYTES * 2);
}

#[test]
fn test_tag_is_stable() {
    assert_eq!(tag("203.0.113.9"), tag("203.0.113.9"));
}

#[test]
fn test_tag_survives_surrounding_whitespace() {
    assert_eq!(tag(" 203.0.113.9 "), tag("203.0.113.9"));
}

#[test]
fn test_tag_differs_per_address() {
    assert_ne!(tag("203.0.113.9"), tag("203.0.113.10"));
    assert_ne!(tag("203.0.113.9"), tag("2001:db8::1"));
}

#[test]
fn test_tag_hides_the_address() {
    assert!(!tag("203.0.113.9").contains("203"));
}

#[test]
fn test_tag_is_not_reversible_by_truncation() {
    // Two addresses sharing a prefix must not collide through the tag.
    assert_ne!(tag("203.0.113.9"), tag("203.0.113.90"));
}
