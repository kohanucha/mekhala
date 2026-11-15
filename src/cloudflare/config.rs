use worker::Env;

pub struct CloudflareConfig {
    pub max_content_length: usize,
    pub max_subscriptions_per_connection: usize,
    pub max_filter_items: usize,
    pub max_event_tags: usize,
    pub max_routing_keys_per_connection: usize,
    pub max_connections: usize,
    pub max_connections_per_ip: usize,
    pub max_connections_per_asn: usize,
    pub idle_timeout_seconds: usize,
    pub connection_sweep_seconds: usize,
    pub evict_min_idle_seconds: usize,
    pub wallet_region: Option<String>,
    pub relay_secret: Option<String>,
}

impl CloudflareConfig {
    pub fn from_env(env: &Env) -> Self {
        Self {
            max_content_length: Self::parse_opt(
                env.var("MAX_CONTENT_LENGTH").ok().map(|v| v.to_string()),
                98_304,
            ),
            max_subscriptions_per_connection: Self::parse_opt(
                env.var("MAX_SUBSCRIPTIONS_PER_CONNECTION")
                    .ok()
                    .map(|v| v.to_string()),
                100,
            ),
            max_filter_items: Self::parse_opt(
                env.var("MAX_FILTER_ITEMS").ok().map(|v| v.to_string()),
                10,
            ),
            max_event_tags: Self::parse_opt(
                env.var("MAX_EVENT_TAGS").ok().map(|v| v.to_string()),
                10,
            ),
            max_routing_keys_per_connection: Self::parse_opt(
                env.var("MAX_ROUTING_KEYS_PER_CONNECTION")
                    .ok()
                    .map(|v| v.to_string()),
                1_000,
            ),
            max_connections: Self::parse_opt(
                env.var("MAX_CONNECTIONS").ok().map(|v| v.to_string()),
                100,
            ),
            max_connections_per_ip: Self::parse_opt(
                env.var("MAX_CONNECTIONS_PER_IP")
                    .ok()
                    .map(|v| v.to_string()),
                5,
            ),
            max_connections_per_asn: Self::parse_opt(
                env.var("MAX_CONNECTIONS_PER_ASN")
                    .ok()
                    .map(|v| v.to_string()),
                20,
            ),
            idle_timeout_seconds: Self::parse_opt(
                env.var("IDLE_TIMEOUT_SECONDS").ok().map(|v| v.to_string()),
                120,
            ),
            connection_sweep_seconds: Self::parse_opt(
                env.var("CONNECTION_SWEEP_SECONDS")
                    .ok()
                    .map(|v| v.to_string()),
                60,
            ),
            evict_min_idle_seconds: Self::parse_opt(
                env.var("EVICT_MIN_IDLE_SECONDS")
                    .ok()
                    .map(|v| v.to_string()),
                10,
            ),
            wallet_region: env
                .var("WALLET_REGION")
                .ok()
                .map(|v| v.to_string())
                .filter(|s| !s.is_empty()),
            relay_secret: env.var("RELAY_SECRET").ok().map(|v| v.to_string()),
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
