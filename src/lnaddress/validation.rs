//! Username shape rules for Lightning Address lookups.
//!
//! The same check guards `.well-known` lookups, callback requests, and (from the
//! registration channel onward) stored registrations, so a name that one of
//! those paths would write is a name the others can resolve.

/// Longest accepted username.
///
/// Lightning Address local parts are short in practice, and this cap keeps the
/// KV key (`lnaddr:<username>`) far below its 512-byte limit while preventing a
/// crafted name from inflating the `metadata` string the wallet hashes.
pub(crate) const MAX_USERNAME_LEN: usize = 64;

/// Whether `s` may be used as a Lightning Address username.
///
/// Only unreserved URL characters are accepted, so the name needs no escaping
/// when it is placed in the callback path. `.` and `..` are refused: as path
/// segments an HTTP client normalises them away, which would produce a callback
/// URL that does not reach this relay's route.
pub(crate) fn is_valid_username(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_USERNAME_LEN
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '~')
}

#[cfg(test)]
#[path = "validation_test.rs"]
mod validation_test;
