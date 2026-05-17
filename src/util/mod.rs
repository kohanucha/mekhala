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
