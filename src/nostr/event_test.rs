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
            Tag::p("recipient1"),
            Tag::p("recipient2"),
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
            Tag::p("pk2"),
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
    let event = Event {
        id: "id5".into(),
        pubkey: "pk1".into(),
        created_at: 1700000000,
        kind: 5,
        tags: vec![Tag::E("event_to_delete".into(), vec![])],
        content: "deleting".into(),
        sig: "badsig".into(),
    };
    let result = event.verify(1700000000, &Limits::default());
    // Kind 5 should pass the kind check, then fail on id/signature
    match result {
        Err(RelayError::InvalidId) | Err(RelayError::InvalidSignature) => {}
        Err(RelayError::InvalidKind) => panic!("kind 5 should not be rejected as InvalidKind"),
        other => panic!("expected id/sig error, got {other:?}"),
    }
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
    let event = Event {
        id: "id1".into(),
        pubkey: "pk1".into(),
        created_at: 1700000000,
        kind: 13194,
        tags: vec![],
        content: "a".repeat(65537),
        sig: "sig".into(),
    };
    let result = event.verify(1700000000, &Limits::default());
    match result {
        Err(RelayError::LimitExceeded(msg)) => assert!(msg.contains("content too large")),
        _ => panic!("expected LimitExceeded, got {result:?}"),
    }
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
        tags: vec![Tag::p("pk2")],
        content: String::new(),
        sig: "sig".into(),
    };
    let result = event.verify(1700000000, &Limits::default());
    assert_eq!(result, Err(RelayError::MissingTag("e".into())));
}

#[test]
fn test_verify_kind_23196_with_p_tag_passes_kind_check() {
    let event = Event {
        id: "id5".into(),
        pubkey: "pk1".into(),
        created_at: 1700000000,
        kind: 23196,
        tags: vec![Tag::p("pk2")],
        content: String::new(),
        sig: "badsig".into(),
    };
    let result = event.verify(1700000000, &Limits::default());
    match result {
        Err(RelayError::InvalidId) | Err(RelayError::InvalidSignature) => {}
        Err(e) => panic!("expected id/sig error for kind 23196 with p tag, got {e:?}"),
        Ok(_) => panic!("expected error for kind 23196 with bad sig"),
    }
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
