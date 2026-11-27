use worker::Env;

pub struct CloudflareConfig {
    pub max_content_length: usize,
    pub max_subscriptions_per_connection: usize,
    pub max_filter_items: usize,
    pub max_filters_per_req: usize,
    pub max_event_tags: usize,
    pub max_routing_keys_per_connection: usize,
    pub max_connections: usize,
    pub max_connections_per_ip: usize,
    pub max_connections_per_asn: usize,
    pub idle_timeout_seconds: usize,
    pub connection_sweep_seconds: usize,
    pub evict_min_idle_seconds: usize,
    pub max_info_events: usize,
    pub info_ttl_seconds: u64,
    pub orphan_sweep_seconds: u64,
    pub wallet_region: Option<String>,
    pub relay_secret: Option<String>,
    pub public_access: bool,
    pub message_rate_limit_per_minute: u32,
}

impl CloudflareConfig {
    pub fn from_env(env: &Env) -> Self {
        Self {
            max_content_length: Self::parse_opt(
                env.var("EVENT_MAX_CONTENT_BYTES")
                    .ok()
                    .map(|v| v.to_string()),
                98_304,
            ),
            max_subscriptions_per_connection: Self::parse_opt(
                env.var("SUB_MAX_PER_CONN").ok().map(|v| v.to_string()),
                100,
            ),
            max_filter_items: Self::parse_opt(
                env.var("SUB_MAX_FILTER_VALUES").ok().map(|v| v.to_string()),
                10,
            ),
            max_filters_per_req: Self::parse_opt(
                env.var("SUB_MAX_FILTERS").ok().map(|v| v.to_string()),
                10,
            ),
            max_event_tags: Self::parse_opt(
                env.var("EVENT_MAX_TAGS").ok().map(|v| v.to_string()),
                10,
            ),
            max_routing_keys_per_connection: Self::parse_opt(
                env.var("SUB_MAX_ROUTING_KEYS").ok().map(|v| v.to_string()),
                1_000,
            ),
            max_connections: Self::parse_opt(
                env.var("CONN_MAX_TOTAL").ok().map(|v| v.to_string()),
                100,
            ),
            max_connections_per_ip: Self::parse_opt(
                env.var("CONN_MAX_PER_IP").ok().map(|v| v.to_string()),
                5,
            ),
            max_connections_per_asn: Self::parse_opt(
                env.var("CONN_MAX_PER_ASN").ok().map(|v| v.to_string()),
                20,
            ),
            idle_timeout_seconds: Self::parse_opt(
                env.var("CONN_IDLE_TIMEOUT_SECONDS")
                    .ok()
                    .map(|v| v.to_string()),
                120,
            ),
            connection_sweep_seconds: Self::parse_opt(
                env.var("CONN_SWEEP_SECONDS").ok().map(|v| v.to_string()),
                60,
            ),
            evict_min_idle_seconds: Self::parse_opt(
                env.var("CONN_EVICT_MIN_IDLE_SECONDS")
                    .ok()
                    .map(|v| v.to_string()),
                10,
            ),
            max_info_events: Self::parse_opt(
                env.var("INFO_CACHE_MAX_EVENTS").ok().map(|v| v.to_string()),
                1_000,
            ),
            info_ttl_seconds: Self::parse_opt_u64(
                env.var("INFO_CACHE_TTL_SECONDS")
                    .ok()
                    .map(|v| v.to_string()),
                2_592_000,
            ),
            orphan_sweep_seconds: Self::parse_opt_u64(
                env.var("CONN_ORPHAN_SWEEP_SECONDS")
                    .ok()
                    .map(|v| v.to_string()),
                3_600,
            ),
            wallet_region: env
                .var("DO_REGION")
                .ok()
                .map(|v| v.to_string())
                .filter(|s| !s.is_empty()),
            relay_secret: env.var("AUTH_RELAY_SECRET").ok().map(|v| v.to_string()),
            public_access: crate::auth::public_flag(
                env.var("AUTH_PUBLIC")
                    .ok()
                    .map(|v| v.to_string())
                    .as_deref(),
            ),
            message_rate_limit_per_minute: Self::parse_opt_u32(
                env.var("CONN_MAX_MESSAGES_PER_MINUTE")
                    .ok()
                    .map(|v| v.to_string()),
                120,
            ),
        }
    }

    fn parse_opt(value: Option<String>, default: usize) -> usize {
        value
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(default)
    }

    fn parse_opt_u64(value: Option<String>, default: u64) -> u64 {
        value.and_then(|s| s.parse::<u64>().ok()).unwrap_or(default)
    }

    fn parse_opt_u32(value: Option<String>, default: u32) -> u32 {
        value.and_then(|s| s.parse::<u32>().ok()).unwrap_or(default)
    }
}

#[cfg(test)]
#[path = "config_test.rs"]
mod config_test;
