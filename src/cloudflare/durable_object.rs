use worker::*;

/// Returns a stub to the relay's Durable Object, optionally hinting the
/// Cloudflare region to place it in (`REGION` values such as `apac`,
/// `weur`, `wnam`).
///
/// # Errors
///
/// Returns an error if the `MEKHALA_NWC_DO` binding is not configured, or
/// if the Durable Object namespace rejects the lookup.
pub fn get_durable_stub(env: &Env, region: Option<&str>) -> Result<Stub> {
    let namespace = env.durable_object("MEKHALA_NWC_DO")?;
    match region {
        Some(r) if !r.is_empty() => namespace.get_by_name_with_location_hint("GLOBAL", r),
        _ => namespace.id_from_name("GLOBAL")?.get_stub(),
    }
}
