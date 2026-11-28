use super::*;
use k256::schnorr::{signature::hazmat::PrehashSigner, SigningKey};

#[test]
fn test_target_pubkeys_author_only() {
    let event = Event {
        id: "id".into(),
        pubkey: "pk1".into(),
        created_at: 0,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: String::new(),
    };
    let keys = event.target_pubkeys();
    assert_eq!(keys.len(), 1);
    assert!(keys.contains("pk1"));
}

#[test]
fn test_target_pubkeys_with_p_tags() {
    let event = Event {
        id: "id".into(),
        pubkey: "author".into(),
        created_at: 0,
        kind: 1,
        tags: vec![
            Tag::P(String::from("recipient1"), vec![]),
            Tag::P(String::from("recipient2"), vec![]),
            Tag::E("event_id".into(), vec![]),
        ],
        content: String::new(),
        sig: String::new(),
    };
    let keys = event.target_pubkeys();
    assert_eq!(keys.len(), 3);
    assert!(keys.contains("author"));
    assert!(keys.contains("recipient1"));
    assert!(keys.contains("recipient2"));
}

#[test]
fn test_target_pubkeys_deduplication() {
    let event = Event {
        id: "id".into(),
        pubkey: "pk1".into(),
        created_at: 0,
        kind: 1,
        tags: vec![
            Tag::P("pk1".into(), vec![]),
            Tag::P(String::from("pk2"), vec![]),
            Tag::P("pk2".into(), vec![]),
        ],
        content: String::new(),
        sig: String::new(),
    };
    let keys = event.target_pubkeys();
    assert_eq!(keys.len(), 2);
    assert!(keys.contains("pk1"));
    assert!(keys.contains("pk2"));
}

#[test]
fn test_kind_5_passes_kind_check() {
    let event = signed(
        &key(1),
        5,
        vec![Tag::E("ab".repeat(32), vec![])],
        "deleting",
    );
    assert_eq!(event.verify(NOW, &Limits::default()), Ok(()));
}

#[test]
fn test_verify_invalid_kind() {
    let event = Event {
        id: "id1".into(),
        pubkey: "pk1".into(),
        created_at: 1700000000,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "sig".into(),
    };
    let result = event.verify(1700000000, &Limits::default());
    assert_eq!(result, Err(RelayError::InvalidKind));
}

#[test]
fn test_verify_content_too_large() {
    let limits = Limits::default();
    let event = Event {
        id: "id1".into(),
        pubkey: "pk1".into(),
        created_at: 1700000000,
        kind: 13194,
        tags: vec![],
        content: "a".repeat(limits.max_content_length + 1),
        sig: "sig".into(),
    };
    let result = event.verify(1700000000, &limits);
    match result {
        Err(RelayError::LimitExceeded(msg)) => assert!(msg.contains("content too large")),
        _ => panic!("expected LimitExceeded, got {result:?}"),
    }
}

/// The largest NIP-44 v2 payload a wallet can produce must not be rejected
/// by the default content ceiling.
#[test]
fn test_verify_default_limit_accepts_max_nip44_payload() {
    let limits = Limits::default();
    let event = Event {
        id: "id1".into(),
        pubkey: "pk1".into(),
        created_at: 1700000000,
        kind: 23194,
        tags: vec![],
        content: "a".repeat(87_472),
        sig: "sig".into(),
    };
    assert!(
        !matches!(
            event.verify(1700000000, &limits),
            Err(RelayError::LimitExceeded(_))
        ),
        "the default limit must accept the largest NIP-44 payload"
    );
}

#[test]
fn test_verify_kind_23196_missing_p_tag() {
    let event = Event {
        id: "id1".into(),
        pubkey: "pk1".into(),
        created_at: 1700000000,
        kind: 23196,
        tags: vec![],
        content: String::new(),
        sig: "sig".into(),
    };
    let result = event.verify(1700000000, &Limits::default());
    assert_eq!(result, Err(RelayError::MissingTag("p".into())));
}

#[test]
fn test_verify_kind_23197_missing_p_tag() {
    let event = Event {
        id: "id2".into(),
        pubkey: "pk1".into(),
        created_at: 1700000000,
        kind: 23197,
        tags: vec![Tag::E("eid1".into(), vec![])],
        content: String::new(),
        sig: "sig".into(),
    };
    let result = event.verify(1700000000, &Limits::default());
    assert_eq!(result, Err(RelayError::MissingTag("p".into())));
}

#[test]
fn test_verify_kind_23195_missing_p_tag() {
    let event = Event {
        id: "id3".into(),
        pubkey: "pk1".into(),
        created_at: 1700000000,
        kind: 23195,
        tags: vec![Tag::E("eid1".into(), vec![])],
        content: String::new(),
        sig: "sig".into(),
    };
    let result = event.verify(1700000000, &Limits::default());
    assert_eq!(result, Err(RelayError::MissingTag("p".into())));
}

#[test]
fn test_verify_kind_23195_missing_e_tag() {
    let event = Event {
        id: "id4".into(),
        pubkey: "pk1".into(),
        created_at: 1700000000,
        kind: 23195,
        tags: vec![Tag::P(String::from("pk2"), vec![])],
        content: String::new(),
        sig: "sig".into(),
    };
    let result = event.verify(1700000000, &Limits::default());
    assert_eq!(result, Err(RelayError::MissingTag("e".into())));
}

#[test]
fn test_verify_kind_23196_with_p_tag_passes_kind_check() {
    let event = signed(&key(1), 23196, vec![Tag::P("cd".repeat(32), vec![])], "");
    assert_eq!(event.verify(NOW, &Limits::default()), Ok(()));
}

#[test]
fn test_verify_valid_signature() {
    let sk_hex = crate::common::test_helpers::TEST_WALLET_SK;
    let pk_hex = crate::common::test_helpers::TEST_WALLET_PK;
    let sk_bytes = hex::decode(sk_hex).unwrap();
    let sk_arr: [u8; 32] = sk_bytes.try_into().unwrap();
    let sk = SigningKey::from_slice(&sk_arr).unwrap();

    let (id, id_bytes) = Event::compute_id(pk_hex, 1700000000, 23194, &[], "test").unwrap();
    let sig = hex::encode(sk.sign_prehash(&id_bytes).unwrap().to_bytes());

    let event = Event {
        id,
        pubkey: pk_hex.into(),
        created_at: 1700000000,
        kind: 23194,
        tags: vec![],
        content: "test".into(),
        sig,
    };
    let result = event.verify(1700000000, &Limits::default());
    assert_eq!(result, Ok(()));
}

#[test]
fn test_verify_created_at_too_old() {
    let now = 1_700_000_000;
    let event = Event {
        id: "id_old".into(),
        pubkey: "pk1".into(),
        created_at: now,
        kind: 13194,
        tags: vec![],
        content: String::new(),
        sig: "sig".into(),
    };
    // One second past the one-year window.
    let result = event.verify(now + 31_536_001, &Limits::default());
    assert_eq!(
        result,
        Err(RelayError::TimestampTooFar(
            "event creation date is too old".into()
        ))
    );
}

#[test]
fn test_verify_pubkey_not_hex() {
    // Compute the id over the same pubkey string so the id check passes and
    // verification reaches the hex decoding step.
    let (id, _) = Event::compute_id("not-hex", 1_700_000_000, 13194, &[], "").unwrap();
    let event = Event {
        id,
        pubkey: "not-hex".into(),
        created_at: 1_700_000_000,
        kind: 13194,
        tags: vec![],
        content: String::new(),
        sig: "00".repeat(64),
    };
    match event.verify(1_700_000_000, &Limits::default()) {
        Err(RelayError::MalformedHex(msg)) => assert!(!msg.is_empty()),
        other => panic!("expected MalformedHex, got {other:?}"),
    }
}

#[test]
fn test_verify_sig_not_hex() {
    let id_pk = crate::common::test_helpers::TEST_WALLET_PK;
    let (id, _) = Event::compute_id(id_pk, 1_700_000_000, 13194, &[], "").unwrap();
    let event = Event {
        id,
        pubkey: id_pk.into(),
        created_at: 1_700_000_000,
        kind: 13194,
        tags: vec![],
        content: String::new(),
        sig: "zz".into(),
    };
    match event.verify(1_700_000_000, &Limits::default()) {
        Err(RelayError::MalformedHex(msg)) => assert!(!msg.is_empty()),
        other => panic!("expected MalformedHex, got {other:?}"),
    }
}

#[test]
fn test_verify_pubkey_wrong_length() {
    // 31 bytes instead of the 32-byte x-only key.
    let short_pk = "11".repeat(31);
    let (id, _) = Event::compute_id(&short_pk, 1_700_000_000, 13194, &[], "").unwrap();
    let event = Event {
        id,
        pubkey: short_pk,
        created_at: 1_700_000_000,
        kind: 13194,
        tags: vec![],
        content: String::new(),
        sig: "00".repeat(64),
    };
    match event.verify(1_700_000_000, &Limits::default()) {
        Err(RelayError::MalformedHex(msg)) => assert_eq!(msg, "public key format"),
        other => panic!("expected MalformedHex(\"public key format\"), got {other:?}"),
    }
}

#[test]
fn test_verify_signature_wrong_length() {
    let pk = crate::common::test_helpers::TEST_WALLET_PK;
    let (id, _) = Event::compute_id(pk, 1_700_000_000, 13194, &[], "").unwrap();
    let event = Event {
        id,
        pubkey: pk.into(),
        created_at: 1_700_000_000,
        kind: 13194,
        tags: vec![],
        content: String::new(),
        sig: "00".repeat(63),
    };
    match event.verify(1_700_000_000, &Limits::default()) {
        Err(RelayError::MalformedHex(msg)) => assert_eq!(msg, "signature format"),
        other => panic!("expected MalformedHex(\"signature format\"), got {other:?}"),
    }
}

#[test]
fn test_compute_id_matches_nip01_golden_vector() {
    // sha256 of `[0,pubkey,created_at,kind,tags,content]`, pinned so a change
    // in serialization cannot silently change every event id.
    let (id, id_bytes) = Event::compute_id(
        crate::common::test_helpers::TEST_WALLET_PK,
        1_700_000_000,
        23194,
        &[],
        "golden",
    )
    .unwrap();
    assert_eq!(
        id,
        "44d56cc9d30fa1b0b4a17b21f21ce6f5e022be1322643316762b0290f0dba640"
    );
    assert_eq!(hex::decode(&id).unwrap(), id_bytes);
}

#[test]
fn test_compute_id_covers_tags_and_content() {
    let (id, _) = Event::compute_id(
        crate::common::test_helpers::TEST_WALLET_PK,
        1_700_000_000,
        23194,
        &[Tag::P(String::from("abc"), vec![])],
        "",
    )
    .unwrap();
    assert_eq!(
        id,
        "3820353c3414592191a2930c3225b31b95d783b040e2018ac8047553f166f59f"
    );
}

#[test]
fn test_verify_content_length_boundary() {
    let limits = Limits::new(4, 100);
    let event = Event {
        id: "id".into(),
        pubkey: "pk".into(),
        created_at: 1_700_000_000,
        kind: 13194,
        tags: vec![],
        content: "abcd".into(),
        sig: "sig".into(),
    };
    // Exactly at the limit passes the size check (and fails later on the id).
    assert!(
        !matches!(
            event.verify(1_700_000_000, &limits),
            Err(RelayError::LimitExceeded(_))
        ),
        "content of exactly max_content_length must be accepted"
    );

    let over = Event {
        content: "abcde".into(),
        ..event
    };
    match over.verify(1_700_000_000, &limits) {
        Err(RelayError::LimitExceeded(msg)) => assert!(msg.contains("content too large")),
        other => panic!("expected LimitExceeded, got {other:?}"),
    }
}

#[test]
fn test_verify_tag_count_boundary() {
    let limits = Limits {
        max_event_tags: 2,
        ..Limits::default()
    };
    let event = Event {
        id: "id".into(),
        pubkey: "pk".into(),
        created_at: 1_700_000_000,
        kind: 13194,
        tags: vec![
            Tag::P(String::from("a"), vec![]),
            Tag::P(String::from("b"), vec![]),
        ],
        content: String::new(),
        sig: "sig".into(),
    };
    // Exactly at the limit passes the tag check (and fails later on the id).
    assert!(
        !matches!(
            event.verify(1_700_000_000, &limits),
            Err(RelayError::LimitExceeded(_))
        ),
        "an event with exactly max_event_tags must be accepted"
    );

    let over = Event {
        tags: vec![
            Tag::P(String::from("a"), vec![]),
            Tag::P(String::from("b"), vec![]),
            Tag::P(String::from("c"), vec![]),
        ],
        ..event
    };
    match over.verify(1_700_000_000, &limits) {
        Err(RelayError::LimitExceeded(msg)) => assert!(msg.contains("too many tags")),
        other => panic!("expected LimitExceeded, got {other:?}"),
    }
}

#[test]
fn test_verify_timestamp_window_boundaries() {
    let limits = Limits::default();
    let now = 1_700_000_000;
    let in_window = Event {
        id: "id".into(),
        pubkey: "pk".into(),
        created_at: now + 900,
        kind: 13194,
        tags: vec![],
        content: String::new(),
        sig: "sig".into(),
    };
    // Exactly +900 s is inside the window; one second more is not.
    assert!(
        !matches!(
            in_window.verify(now, &limits),
            Err(RelayError::TimestampTooFar(_))
        ),
        "created_at == now + 900 must be accepted"
    );
    let future = Event {
        created_at: now + 901,
        ..in_window
    };
    match future.verify(now, &limits) {
        Err(RelayError::TimestampTooFar(msg)) => assert!(msg.contains("too far off")),
        other => panic!("expected TimestampTooFar, got {other:?}"),
    }

    let oldest = Event {
        id: "id".into(),
        pubkey: "pk".into(),
        created_at: now - 31_536_000,
        kind: 13194,
        tags: vec![],
        content: String::new(),
        sig: "sig".into(),
    };
    // Exactly one year old is still accepted; one second older is not.
    assert!(
        !matches!(
            oldest.verify(now, &limits),
            Err(RelayError::TimestampTooFar(_))
        ),
        "created_at == now - 31_536_000 must be accepted"
    );
    let too_old = Event {
        created_at: now - 31_536_001,
        ..oldest
    };
    match too_old.verify(now, &limits) {
        Err(RelayError::TimestampTooFar(msg)) => assert!(msg.contains("too old")),
        other => panic!("expected TimestampTooFar, got {other:?}"),
    }
}

/// An info event padded so its stored JSON is exactly `len` bytes.
fn info_event_of_len(len: usize) -> Event {
    let mut event = Event {
        id: "id".into(),
        pubkey: "pk".into(),
        created_at: 1_700_000_000,
        kind: 13194,
        tags: vec![],
        content: String::new(),
        sig: "sig".into(),
    };
    event.content = "a".repeat(len - event.json_len());
    assert_eq!(event.json_len(), len);
    event
}

#[test]
fn test_verify_info_event_size_boundary() {
    let limits = Limits::default();
    // At the limit passes the size check (and fails later on the id).
    assert!(!matches!(
        info_event_of_len(MAX_INFO_EVENT_BYTES).verify(1_700_000_000, &limits),
        Err(RelayError::LimitExceeded(_))
    ));
    match info_event_of_len(MAX_INFO_EVENT_BYTES + 1).verify(1_700_000_000, &limits) {
        Err(RelayError::LimitExceeded(msg)) => assert!(msg.contains("info event too large")),
        other => panic!("expected LimitExceeded, got {other:?}"),
    }
}

#[test]
fn test_verify_size_limit_applies_only_to_info_events() {
    // An NWC request carries NIP-44 content far larger than an info event.
    let request = Event {
        kind: 23194,
        tags: vec![Tag::P(String::from("wallet"), vec![])],
        ..info_event_of_len(MAX_INFO_EVENT_BYTES * 4)
    };
    assert!(!matches!(
        request.verify(1_700_000_000, &Limits::default()),
        Err(RelayError::LimitExceeded(_))
    ));
}

/// An event whose id the client computed over tags the relay would rewrite
/// (a numeric `expiration`, extra `encryption` values) must still verify.
#[test]
fn test_verify_uses_tags_exactly_as_signed() {
    let pk_hex = crate::common::test_helpers::TEST_WALLET_PK;
    let sk_bytes: [u8; 32] = hex::decode(crate::common::test_helpers::TEST_WALLET_SK)
        .unwrap()
        .try_into()
        .unwrap();
    let sk = SigningKey::from_slice(&sk_bytes).unwrap();

    // Computed the way a client does: over the JSON it sends, not over `Tag`.
    let tags = serde_json::json!([
        ["p", pk_hex],
        ["expiration", 1_700_000_600u64],
        ["encryption", "nip44_v2", "extra"]
    ]);
    let preimage = serde_json::to_string(&serde_json::json!([
        0,
        pk_hex,
        1_700_000_000u64,
        23194,
        tags,
        "x"
    ]))
    .unwrap();
    let id_bytes: [u8; 32] = Sha256::digest(preimage.as_bytes()).into();
    let sig = hex::encode(sk.sign_prehash(&id_bytes).unwrap().to_bytes());

    let event: Event = serde_json::from_value(serde_json::json!({
        "id": hex::encode(id_bytes),
        "pubkey": pk_hex,
        "created_at": 1_700_000_000u64,
        "kind": 23194,
        "tags": tags,
        "content": "x",
        "sig": sig,
    }))
    .unwrap();
    assert_eq!(event.verify(1_700_000_000, &Limits::default()), Ok(()));
}

const NOW: u64 = 1_700_000_000;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_slice(&[seed; 32]).unwrap()
}

/// An event signed by `sk`, with the id and signature a client would send.
fn signed(sk: &SigningKey, kind: u64, tags: Vec<Tag>, content: &str) -> Event {
    let pubkey = hex::encode(sk.verifying_key().to_bytes());
    let (id, id_bytes) = Event::compute_id(&pubkey, NOW, kind, &tags, content).unwrap();
    let sig = hex::encode(sk.sign_prehash(&id_bytes).unwrap().to_bytes());
    Event {
        id,
        pubkey,
        created_at: NOW,
        kind,
        tags,
        content: content.into(),
        sig,
    }
}

/// The id matches the content and the author, but another key signed it:
/// only the signature check can refuse this event.
#[test]
fn test_signature_by_another_key_is_refused() {
    let mut event = signed(&key(1), 13194, vec![], "pay_invoice");
    let (_, id_bytes) = Event::compute_id(
        &event.pubkey,
        event.created_at,
        event.kind,
        &event.tags,
        &event.content,
    )
    .unwrap();
    event.sig = hex::encode(key(2).sign_prehash(&id_bytes).unwrap().to_bytes());

    assert_eq!(
        event.verify(NOW, &Limits::default()),
        Err(RelayError::InvalidSignature)
    );
}

#[test]
fn test_one_flipped_signature_byte_is_refused() {
    let mut event = signed(&key(1), 23194, vec![Tag::P("cd".repeat(32), vec![])], "x");
    let mut sig = hex::decode(&event.sig).unwrap();
    sig[63] ^= 0x01;
    event.sig = hex::encode(sig);

    assert_eq!(
        event.verify(NOW, &Limits::default()),
        Err(RelayError::InvalidSignature)
    );
}

/// A correctly signed event whose content was changed afterwards: the id no
/// longer matches, and that is the error reported.
#[test]
fn test_content_changed_after_signing_is_an_id_mismatch() {
    let mut event = signed(&key(1), 13194, vec![], "pay_invoice");
    event.content = "pay_invoice make_invoice".into();

    assert_eq!(
        event.verify(NOW, &Limits::default()),
        Err(RelayError::InvalidId)
    );
}
