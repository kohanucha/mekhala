use worker::Env;
use crate::auth::AccessPolicy;

pub fn from_env(env: &Env) -> AccessPolicy {
    AccessPolicy::new(
        env.var("RELAY_SECRET").map(|v| v.to_string()).ok()
    )
}
