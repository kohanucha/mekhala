use worker::*;

pub fn get_durable_stub(env: &Env, region: Option<String>) -> Result<Stub> {
    let namespace = env.durable_object("NWC_RELAY")?;
    match region {
        Some(r) if !r.is_empty() => namespace.get_by_name_with_location_hint("GLOBAL", &r),
        _ => namespace.id_from_name("GLOBAL")?.get_stub(),
    }
}