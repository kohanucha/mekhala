//! Utility functions: timestamp, string truncation, logging macros.
//!
//! All functions are safe for wasm ("workers-js") and native test builds;
//! log macros compile to `console_*` calls only on wasm and are no-ops
//! natively.

/// Current time as seconds since the Unix epoch.
pub fn now() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (worker::Date::now().as_millis() / 1000) as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_secs(),
            Err(_) => 0,
        }
    }
}

/// Truncates `s` to at most `len` bytes for log display.
///
/// Intended for hex identifiers and other ASCII strings; callers measure
/// with `s.len()`, so the argument is a byte budget, not a character count.
///
/// # Panics
///
/// Panics when `len` falls inside a multi-byte UTF-8 character and `s`
/// contains non-ASCII text. Callers today pass hex/ASCII identifiers only;
/// do not use on arbitrary user text without a char-boundary check.
pub fn short(s: &str, len: usize) -> &str {
    if s.len() <= len { s } else { &s[..len] }
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        #[cfg(target_arch = "wasm32")]
        { worker::console_log!($($arg)*); }
        #[cfg(not(target_arch = "wasm32"))]
        { let _ = format_args!($($arg)*); }
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        #[cfg(target_arch = "wasm32")]
        { worker::console_debug!($($arg)*); }
        #[cfg(not(target_arch = "wasm32"))]
        { let _ = format_args!($($arg)*); }
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        #[cfg(target_arch = "wasm32")]
        { worker::console_warn!($($arg)*); }
        #[cfg(not(target_arch = "wasm32"))]
        { let _ = format_args!($($arg)*); }
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        #[cfg(target_arch = "wasm32")]
        { worker::console_error!($($arg)*); }
        #[cfg(not(target_arch = "wasm32"))]
        { let _ = format_args!($($arg)*); }
    };
}

#[cfg(test)]
#[path = "util_test.rs"]
mod util_test;
