use crate::domain::{Event, Filter, RelayError};

/// Nostr Event Kinds for NWC
pub const KIND_NWC_INFO: u64 = 13194;
pub const KIND_NWC_REQUEST: u64 = 23194;
pub const KIND_NWC_RESPONSE: u64 = 23195;
pub const KIND_NWC_NOTIFICATION_1: u64 = 23196;
pub const KIND_NWC_NOTIFICATION_2: u64 = 23197;

pub const ALLOWED_KINDS: &[u64] = &[
    KIND_NWC_INFO,
    KIND_NWC_REQUEST,
    KIND_NWC_RESPONSE,
    KIND_NWC_NOTIFICATION_1,
    KIND_NWC_NOTIFICATION_2,
];

/// Domain-specific validation for NWC protocol.
pub struct NwcProtocol;

impl NwcProtocol {
    /// Validates that the event matches Mekhala's NWC-specific rules.
    pub fn validate_event(event: &Event) -> Result<(), RelayError> {
        // 1. Verify Allowed Kinds
        if !ALLOWED_KINDS.contains(&event.kind) {
            return Err(RelayError::InvalidKind);
        }

        // 2. Verify NIP-47 constraints (strict tag enforcement)
        match event.kind {
            KIND_NWC_REQUEST | KIND_NWC_NOTIFICATION_1 | KIND_NWC_NOTIFICATION_2 => {
                if !event.has_tag("p") {
                    return Err(RelayError::MissingTag("#p tag".into()));
                }
            }
            KIND_NWC_RESPONSE => {
                if !event.has_tag("p") || !event.has_tag("e") {
                    return Err(RelayError::MissingTag("#p or #e tag".into()));
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Validates that the filter is narrowed enough and restricted to NWC kinds.
    pub fn validate_filter(filter: &Filter) -> bool {
        // 1. Protocol strictness: Require NWC kinds
        let kinds = match &filter.kinds {
            Some(k) if !k.is_empty() => k,
            _ => return false, // Kinds MUST be present and not empty
        };

        // All requested kinds MUST be NWC kinds
        if kinds.iter().any(|k| !ALLOWED_KINDS.contains(k)) {
            return false;
        }

        // 2. Privacy + Performance: Require narrowing for sensitive kinds
        let has_narrowing = filter.authors.as_ref().map_or(false, |v| !v.is_empty())
            || filter.p_tags.as_ref().map_or(false, |v| !v.is_empty())
            || filter.e_tags.as_ref().map_or(false, |v| !v.is_empty())
            || filter.ids.as_ref().map_or(false, |v| !v.is_empty());

        has_narrowing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_validate_filter_nwc_strict() {
        // Must have kinds
        let f_no_kinds = Filter {
            authors: Some(vec!["a".into()]),
            ..Default::default()
        };
        assert!(!NwcProtocol::validate_filter(&f_no_kinds));

        // Must be NWC kinds
        let f_invalid_kind = Filter {
            kinds: Some(vec![1]),
            authors: Some(vec!["a".into()]),
            ..Default::default()
        };
        assert!(!NwcProtocol::validate_filter(&f_invalid_kind));

        // Must have narrowing
        let f_no_narrowing = Filter {
            kinds: Some(vec![KIND_NWC_REQUEST]),
            ..Default::default()
        };
        assert!(!NwcProtocol::validate_filter(&f_no_narrowing));

        // Valid
        let f_valid = Filter {
            kinds: Some(vec![KIND_NWC_REQUEST]),
            authors: Some(vec!["a".into()]),
            ..Default::default()
        };
        assert!(NwcProtocol::validate_filter(&f_valid));
    }

    #[test]
    fn test_validate_event_nwc_rules() {
        let mut event = Event {
            id: "id".into(),
            pubkey: "pub".into(),
            created_at: 100,
            kind: KIND_NWC_REQUEST,
            tags: vec![],
            content: "content".into(),
            sig: "sig".into(),
        };

        // Fail: Missing p-tag for REQUEST
        assert!(matches!(NwcProtocol::validate_event(&event), Err(RelayError::MissingTag(_))));

        // Pass: Has p-tag
        event.tags = vec![vec![json!("p"), json!("pub")]];
        assert!(NwcProtocol::validate_event(&event).is_ok());

        // Fail: Invalid kind
        event.kind = 1;
        assert!(matches!(NwcProtocol::validate_event(&event), Err(RelayError::InvalidKind)));
    }
}
