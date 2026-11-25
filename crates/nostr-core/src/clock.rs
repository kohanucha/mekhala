//! Wall-clock time for both build targets.
//!
//! `std::time::SystemTime::now()` panics on `wasm32-unknown-unknown`, so the
//! Worker build reads the JavaScript clock instead. Native builds (tests, a
//! self-hosted service) use the system clock.

/// Current time as seconds since the Unix epoch.
#[cfg(target_arch = "wasm32")]
pub fn now() -> u64 {
    // `Date.now()` is milliseconds as an `f64`; it is never negative and stays
    // far below `u64::MAX`, so the truncating cast is exact for whole seconds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let seconds = (js_sys::Date::now() / 1000.0) as u64;
    seconds
}

/// Current time as seconds since the Unix epoch.
#[cfg(not(target_arch = "wasm32"))]
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
#[path = "clock_test.rs"]
mod clock_test;
