use worker::*;

pub struct CloudflareConfig {
    pub max_content_length: usize,
    pub max_subscriptions_per_connection: usize,
    pub max_connections: usize,
    pub wallet_region: Option<String>,
    pub relay_secret: Option<String>,
}

impl CloudflareConfig {
    pub fn from_env(env: &Env) -> Self {
        Self {
            max_content_length: Self::parse_opt(
                env.var("MAX_CONTENT_LENGTH").ok().map(|v| v.to_string()),
                65536,
            ),
            max_subscriptions_per_connection: Self::parse_opt(
                env.var("MAX_SUBSCRIPTIONS_PER_CONNECTION").ok().map(|v| v.to_string()),
                100,
            ),
            max_connections: Self::parse_opt(
                env.var("MAX_CONNECTIONS").ok().map(|v| v.to_string()),
                100,
            ),
            wallet_region: env.var("WALLET_REGION").ok()
                .map(|v| v.to_string())
                .filter(|s| !s.is_empty()),
            relay_secret: env.var("RELAY_SECRET").ok()
                .map(|v| v.to_string()),
        }
    }

    fn parse_opt(value: Option<String>, default: usize) -> usize {
        value
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(default)
    }
}

#[cfg(test)]
#[path = "config_test.rs"]
mod config_test;
