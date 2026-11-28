use crate::auth::AccessPolicy;
use crate::cloudflare::config::CloudflareConfig;

pub fn from_config(config: &CloudflareConfig) -> AccessPolicy {
    AccessPolicy::new(config.relay_secret.clone())
}
