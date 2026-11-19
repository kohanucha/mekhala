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
    pub max_info_events: usize,
    pub info_ttl_seconds: u64,
    pub wallet_region: Option<String>,
    pub relay_secret: Option<String>,
    pub min_amount_msat: u64,
    pub max_amount_msat: u64,
    pub callback_rate_limit_per_minute: u32,
    pub rate_limit_max_keys: usize,
    pub ln_address_keys: crate::lnaddress::LnAddressKeys,
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
            max_info_events: Self::parse_opt(
                env.var("MAX_INFO_EVENTS").ok().map(|v| v.to_string()),
                1_000,
            ),
            info_ttl_seconds: Self::parse_opt_u64(
                env.var("INFO_TTL_SECONDS").ok().map(|v| v.to_string()),
                2_592_000,
            ),
            wallet_region: env
                .var("WALLET_REGION")
                .ok()
                .map(|v| v.to_string())
                .filter(|s| !s.is_empty()),
            relay_secret: env.var("RELAY_SECRET").ok().map(|v| v.to_string()),
            min_amount_msat: Self::parse_opt_u64(
                env.var("MIN_AMOUNT_MSAT").ok().map(|v| v.to_string()),
                crate::lnaddress::policy::DEFAULT_MIN_SENDABLE_MSAT,
            ),
            max_amount_msat: Self::parse_opt_u64(
                env.var("MAX_AMOUNT_MSAT").ok().map(|v| v.to_string()),
                crate::lnaddress::policy::DEFAULT_MAX_SENDABLE_MSAT,
            ),
            callback_rate_limit_per_minute: Self::parse_opt_u32(
                env.var("CALLBACK_RATE_LIMIT_PER_MINUTE")
                    .ok()
                    .map(|v| v.to_string()),
                10,
            ),
            rate_limit_max_keys: Self::parse_opt(
                env.var("RATE_LIMIT_MAX_KEYS").ok().map(|v| v.to_string()),
                1_000,
            ),
            ln_address_keys: crate::lnaddress::LnAddressKeys::from_hex(
                env.var(crate::lnaddress::record::KEY_ENV)
                    .ok()
                    .map(|v| v.to_string()),
                env.var(crate::lnaddress::record::PREVIOUS_KEY_ENV)
                    .ok()
                    .map(|v| v.to_string()),
            ),
        }
    }

    /// The LN address sendable range and callback rate limit. Both the
    /// advertised `payRequest` and the callback read this, so the range a
    /// client is shown is the range that is enforced.
    pub fn lnaddress_policy(&self) -> crate::lnaddress::LnAddressPolicy {
        crate::lnaddress::LnAddressPolicy::new(
            self.min_amount_msat,
            self.max_amount_msat,
            self.callback_rate_limit_per_minute,
        )
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
