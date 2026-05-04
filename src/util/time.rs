pub fn now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        worker::Date::now().as_millis()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

pub fn now() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (worker::Date::now().as_millis() / 1000) as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}
