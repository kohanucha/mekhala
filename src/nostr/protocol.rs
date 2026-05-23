//! Protocol constants for NWC event kinds and validation.

/// Event kinds permitted on this relay (NIP-47 NWC + NIP-09 deletion).
pub const ALLOWED_EVENT_KINDS: &[u64] = &[5, 13194, 23194, 23195, 23196, 23197];

/// Check whether a kind is allowed as an event on this relay.
pub fn is_allowed_event_kind(kind: u64) -> bool {
    ALLOWED_EVENT_KINDS.iter().any(|&k| k == kind)
}

/// Check whether a kind is allowed in a subscription filter on this relay.
pub fn is_allowed_filter_kind(kind: &u64) -> bool {
    ALLOWED_EVENT_KINDS.iter().any(|&k| k == *kind)
}
