//! Protocol constants for NWC event kinds and validation.

/// NIP-09 deletion event kind.
pub const KIND_DELETION: u64 = 5;
/// NIP-47 wallet info event kind.
pub const KIND_NWC_INFO: u64 = 13194;
/// NIP-47 NWC request event kind.
pub const KIND_NWC_REQUEST: u64 = 23194;
/// NIP-47 NWC response event kind.
pub const KIND_NWC_RESPONSE: u64 = 23195;
/// NIP-47 NWC client message event kind.
pub const KIND_NWC_CLIENT_MSG: u64 = 23196;
/// NIP-47 NWC client feedback event kind.
pub const KIND_NWC_CLIENT_FEEDBACK: u64 = 23197;

/// All event kinds permitted on this relay.
pub const ALLOWED_EVENT_KINDS: &[u64] = &[
    KIND_DELETION,
    KIND_NWC_INFO,
    KIND_NWC_REQUEST,
    KIND_NWC_RESPONSE,
    KIND_NWC_CLIENT_MSG,
    KIND_NWC_CLIENT_FEEDBACK,
];

/// Check whether a kind is allowed as an event on this relay.
pub fn is_allowed_event_kind(kind: u64) -> bool {
    ALLOWED_EVENT_KINDS.iter().any(|&k| k == kind)
}

/// Check whether a kind is allowed in a subscription filter on this relay.
pub fn is_allowed_filter_kind(kind: &u64) -> bool {
    ALLOWED_EVENT_KINDS.iter().any(|&k| k == *kind)
}
