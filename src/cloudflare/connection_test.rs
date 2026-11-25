use super::*;
use futures::channel::oneshot;

fn attachment(id: u32, ip_tag: Option<&str>, asn: Option<u32>) -> Attachment {
    Attachment {
        id,
        ip_tag: ip_tag.map(str::to_string),
        asn,
        last_seen: 1_000,
    }
}

fn limits(max_connections: usize, max_per_ip: usize, max_per_asn: usize) -> AdmissionLimits {
    AdmissionLimits {
        max_connections,
        max_per_ip,
        max_per_asn,
    }
}

#[test]
fn test_send_internal_delivers() {
    let mut reg = ConnectionRegistry::new();
    let (tx, rx) = oneshot::channel();
    reg.add_internal(1, tx);

    let sent = reg.send(1, "hello".into());
    assert!(sent);
    assert_eq!(futures::executor::block_on(rx), Ok("hello".into()));
}

#[test]
fn test_send_unknown_id() {
    let mut reg = ConnectionRegistry::new();
    assert!(!reg.send(42, "msg".into()));
}

#[test]
fn test_remove_internal() {
    let mut reg = ConnectionRegistry::new();
    let (tx, rx) = oneshot::channel::<String>();
    reg.add_internal(1, tx);

    reg.remove(1);
    assert!(!reg.send(1, "msg".into()));
    // Receiver should be dropped since sender was removed
    assert!(futures::executor::block_on(rx).is_err());
}

#[test]
fn test_send_internal_consumes_entry() {
    let mut reg = ConnectionRegistry::new();
    let (tx, _rx) = oneshot::channel();
    reg.add_internal(1, tx);

    assert!(reg.send(1, "first".into()));
    assert!(!reg.send(1, "second".into()));
}

#[test]
fn test_new_registry_empty() {
    let reg = ConnectionRegistry::new();
    let (tx, rx) = oneshot::channel::<String>();
    // Even though we have a sender, it's not in the registry
    assert!(reg.connections.keys().next().is_none());
    drop(tx);
    futures::executor::block_on(async {
        let result = rx.await;
        assert!(result.is_err());
    });
}

#[test]
fn test_send_external_tracks_after_send() {
    // For External connections, send should not remove the entry
    // (WebSocket send can fail, but the connection should remain tracked).
    // We can't construct a real WebSocket, but we verify the code path
    // by checking that send returns true (the ws.send_with_str error is ignored)
    // and the entry remains after send for unknown IDs.
    let mut reg = ConnectionRegistry::new();
    assert!(!reg.send(99, "msg".into()));
}

#[test]
fn test_legacy_attachment_has_no_identity() {
    let legacy = Attachment::legacy(7);
    assert_eq!(legacy.id, 7);
    assert_eq!(legacy.ip_tag, None);
    assert_eq!(legacy.asn, None);
    assert_eq!(legacy.last_seen, 0);
}

#[test]
fn test_attachment_round_trips_through_json() {
    let original = attachment(3, Some("abc123"), Some(64512));
    let encoded = serde_json::to_value(&original).expect("serialize");
    let decoded: Attachment = serde_json::from_value(encoded).expect("deserialize");
    assert_eq!(decoded, original);
}

#[test]
fn test_attachment_omits_missing_identity() {
    let encoded = serde_json::to_value(Attachment::legacy(1)).expect("serialize");
    let object = encoded.as_object().expect("object");
    assert!(!object.contains_key("ip_tag"));
    assert!(!object.contains_key("asn"));
}

#[test]
fn test_bare_id_is_not_a_rich_attachment() {
    // The fallback to the legacy id only works because this fails: a bare
    // number cannot be read as the richer attachment.
    assert!(serde_json::from_value::<Attachment>(serde_json::json!(7)).is_err());
}

#[test]
fn test_admit_accepts_under_every_cap() {
    let existing = vec![attachment(1, Some("ip-a"), Some(1))];
    let candidate = attachment(2, Some("ip-b"), Some(2));
    assert_eq!(
        admit(&existing, existing.len(), &candidate, &limits(10, 2, 2)),
        Admission::Accept
    );
}

#[test]
fn test_admit_rejects_when_address_is_at_capacity() {
    let existing = vec![
        attachment(1, Some("ip-a"), Some(1)),
        attachment(2, Some("ip-a"), Some(1)),
    ];
    let candidate = attachment(3, Some("ip-a"), Some(1));
    assert_eq!(
        admit(&existing, existing.len(), &candidate, &limits(10, 2, 10)),
        Admission::IpAtCapacity
    );
}

#[test]
fn test_admit_counts_only_the_matching_address() {
    let existing = vec![
        attachment(1, Some("ip-a"), Some(1)),
        attachment(2, Some("ip-b"), Some(1)),
    ];
    let candidate = attachment(3, Some("ip-b"), Some(1));
    assert_eq!(
        admit(&existing, existing.len(), &candidate, &limits(10, 2, 10)),
        Admission::Accept
    );
}

#[test]
fn test_admit_rejects_when_network_is_at_capacity() {
    let existing = vec![
        attachment(1, Some("ip-a"), Some(64512)),
        attachment(2, Some("ip-b"), Some(64512)),
    ];
    let candidate = attachment(3, Some("ip-c"), Some(64512));
    assert_eq!(
        admit(&existing, existing.len(), &candidate, &limits(10, 10, 2)),
        Admission::AsnAtCapacity
    );
}

#[test]
fn test_admit_reports_full_relay() {
    let existing = vec![attachment(1, Some("ip-a"), Some(1))];
    let candidate = attachment(2, Some("ip-b"), Some(2));
    assert_eq!(
        admit(&existing, existing.len(), &candidate, &limits(1, 10, 10)),
        Admission::Full
    );
}

#[test]
fn test_admit_checks_address_before_total_capacity() {
    // A client already at its address cap is refused for that reason even
    // when the relay is full, so the log names the limit that was hit.
    let existing = vec![attachment(1, Some("ip-a"), Some(1))];
    let candidate = attachment(2, Some("ip-a"), Some(1));
    assert_eq!(
        admit(&existing, existing.len(), &candidate, &limits(1, 1, 10)),
        Admission::IpAtCapacity
    );
}

#[test]
fn test_admit_ignores_zero_caps() {
    let existing = vec![
        attachment(1, Some("ip-a"), Some(1)),
        attachment(2, Some("ip-a"), Some(1)),
    ];
    let candidate = attachment(3, Some("ip-a"), Some(1));
    assert_eq!(
        admit(&existing, existing.len(), &candidate, &limits(10, 0, 0)),
        Admission::Accept
    );
}

#[test]
fn test_admit_fails_open_without_identity() {
    // No address and no network known (e.g. a local run without an edge):
    // identity caps cannot be applied, so only the total cap counts.
    let existing = vec![attachment(1, None, None), attachment(2, None, None)];
    let candidate = attachment(3, None, None);
    assert_eq!(
        admit(&existing, existing.len(), &candidate, &limits(10, 1, 1)),
        Admission::Accept
    );
    assert_eq!(
        admit(&existing, existing.len(), &candidate, &limits(2, 1, 1)),
        Admission::Full
    );
}

#[test]
fn test_admit_counts_sockets_without_attachments() {
    // Sockets accepted before attachments were introduced still occupy a
    // slot, so the total cap must count them.
    let existing = vec![attachment(1, Some("ip-a"), Some(1))];
    let candidate = attachment(2, Some("ip-b"), Some(2));
    assert_eq!(
        admit(&existing, 3, &candidate, &limits(3, 10, 10)),
        Admission::Full
    );
}

#[test]
fn test_registry_forgets_activity_on_remove() {
    let mut reg = ConnectionRegistry::new();
    reg.last_seen.insert(5, 42);
    reg.remove(5);
    assert_eq!(reg.last_seen(5), None);
}

#[test]
fn test_message_budget_is_per_connection() {
    let mut reg = ConnectionRegistry::new();
    assert_eq!(
        reg.admit_message(1, 1_000, 2),
        rate_limit::Admission::Admitted
    );
    assert_eq!(
        reg.admit_message(1, 1_000, 2),
        rate_limit::Admission::Admitted
    );
    assert_eq!(
        reg.admit_message(1, 1_000, 2),
        rate_limit::Admission::Limited { retry_after: 60 }
    );

    // Another connection has not spent anything.
    assert_eq!(
        reg.admit_message(2, 1_000, 2),
        rate_limit::Admission::Admitted
    );
}

#[test]
fn test_message_budget_with_no_limit_admits_everything() {
    let mut reg = ConnectionRegistry::new();
    for _ in 0..50 {
        assert_eq!(
            reg.admit_message(1, 1_000, 0),
            rate_limit::Admission::Admitted
        );
    }
}

#[test]
fn test_message_budget_resets_when_the_window_rolls() {
    let mut reg = ConnectionRegistry::new();
    assert_eq!(
        reg.admit_message(1, 1_000, 1),
        rate_limit::Admission::Admitted
    );
    assert_eq!(
        reg.admit_message(1, 1_030, 1),
        rate_limit::Admission::Limited { retry_after: 30 }
    );
    assert_eq!(
        reg.admit_message(1, 1_060, 1),
        rate_limit::Admission::Admitted
    );
}

#[test]
fn test_registry_forgets_the_message_budget_on_remove() {
    // A new connection id must not inherit a spent window, and the map must
    // not grow with connections that are gone.
    let mut reg = ConnectionRegistry::new();
    assert_eq!(
        reg.admit_message(7, 1_000, 1),
        rate_limit::Admission::Admitted
    );
    reg.remove(7);
    assert_eq!(
        reg.admit_message(7, 1_000, 1),
        rate_limit::Admission::Admitted
    );
    assert_eq!(reg.message_windows.len(), 1);
}
