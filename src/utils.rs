use worker::*;

pub fn set_panic_hook() {
    // When the `console_error_panic_hook` feature is enabled, we can call this
    // function at least once during initialization, and then we will get better
    // error messages if our code ever panics.
    //
    // For more details see
    // https://github.com/rustwasm/console_error_panic_hook#readme
    console_error_panic_hook::set_once();
}

pub fn create_cors_response(response: Response) -> Result<Response> {
    let headers = response.headers().clone();
    headers.set("Access-Control-Allow-Origin", "*")?;
    headers.set("Access-Control-Allow-Methods", "GET, OPTIONS")?;
    headers.set("Access-Control-Allow-Headers", "*")?;
    headers.set("Content-Type", "application/nostr+json")?;
    Ok(response.with_headers(headers))
}

/// Constant-time string comparison to prevent timing attacks
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() { return false; }
    a.bytes().zip(b.bytes()).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Centralized Durable Object stub retrieval
pub fn get_durable_stub(env: &Env, region: Option<String>) -> Result<Stub> {
    let namespace = env.durable_object("NWC_RELAY")?;
    match region {
        Some(r) if !r.is_empty() => namespace.get_by_name_with_location_hint("GLOBAL", &r),
        _ => namespace.id_from_name("GLOBAL")?.get_stub(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq("secret", "secret"));
        assert!(!constant_time_eq("secret", "wrong!!"));
        assert!(!constant_time_eq("secret", "secre"));
        assert!(!constant_time_eq("secret", "secrets"));
        assert!(constant_time_eq("", ""));
    }
}
