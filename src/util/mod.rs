//! Utility functions: timestamp, string truncation, logging macros.

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
