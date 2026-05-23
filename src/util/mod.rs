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
macro_rules! _log_impl {
    ($console_fn:path, $($arg:tt)*) => {
        #[cfg(target_arch = "wasm32")]
        { $console_fn($($arg)*); }
        #[cfg(not(target_arch = "wasm32"))]
        { let _ = format_args!($($arg)*); }
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { crate::_log_impl!(worker::console_log, $($arg)*) };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => { crate::_log_impl!(worker::console_debug, $($arg)*) };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { crate::_log_impl!(worker::console_warn, $($arg)*) };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { crate::_log_impl!(worker::console_error, $($arg)*) };
}
