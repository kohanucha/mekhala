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

/// A helper trait to decode a hex string into binary bytes with consistent, clear error handling.
pub trait FromHexStr {
    fn decode_hex(&self) -> crate::nostr::Result<Vec<u8>>;
}

impl FromHexStr for str {
    fn decode_hex(&self) -> crate::nostr::Result<Vec<u8>> {
        hex::decode(self).map_err(|e| crate::nostr::RelayError::MalformedHex(e.to_string()))
    }
}

impl FromHexStr for String {
    fn decode_hex(&self) -> crate::nostr::Result<Vec<u8>> {
        self.as_str().decode_hex()
    }
}

/// A helper trait to encode byte slices/collections into a hex string.
pub trait ToHex {
    fn to_hex(&self) -> String;
}

impl<T: AsRef<[u8]>> ToHex for T {
    fn to_hex(&self) -> String {
        hex::encode(self)
    }
}
