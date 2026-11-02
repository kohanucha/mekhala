use super::*;
use crate::common::test_helpers::{TEST_NWC_URI, TEST_WALLET_PK, TEST_WALLET_SK};

#[test]
fn test_nwc_uri_from_uri() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    assert_eq!(nwc_uri.wallet_pubkey, TEST_WALLET_PK);
    assert_eq!(nwc_uri.secret, TEST_WALLET_SK);
}

#[test]
fn test_nwc_client_roundtrip() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let client = NwcClient::new(nwc_uri).unwrap();

    let payload = serde_json::json!({"test": "data"});
    let encrypted = client.encrypt(&payload).unwrap();
    let decrypted = client.decrypt(&encrypted).unwrap();
    let decrypted_json: Value = serde_json::from_str(&decrypted).unwrap();
    assert_eq!(payload, decrypted_json);

    let (event, request_id) = client
        .create_request_event(NwcMethod::MakeInvoice, payload, vec![])
        .unwrap();
    assert_eq!(event.pubkey, client.my_pubkey);
    assert_eq!(event.kind, KIND_NWC_REQUEST);

    // Test parsing back (mocking a response)
    let resp_payload = serde_json::json!({"result": {"invoice": "lnbc1..."}});
    let resp_encrypted = client.encrypt(&resp_payload).unwrap();
    let resp_event = Event {
        id: "resp_id".into(),
        pubkey: client.wallet_pubkey.clone(),
        created_at: crate::util::now(),
        kind: 23195,
        tags: vec![
            Tag::E(request_id.clone(), vec![]),
            Tag::p(&client.my_pubkey),
        ],
        content: resp_encrypted,
        sig: "sig".into(),
    };

    let wallet_sk_bytes = hex::decode(TEST_WALLET_SK).unwrap();
    let wallet_sk_arr: [u8; 32] = wallet_sk_bytes.try_into().unwrap();
    let wallet_sk = SigningKey::from_slice(&wallet_sk_arr).unwrap();

    let (resp_id, resp_id_bytes) = Event::compute_id(
        &client.wallet_pubkey,
        resp_event.created_at,
        resp_event.kind,
        &resp_event.tags,
        &resp_event.content,
    )
    .unwrap();
    let resp_sig = hex::encode(wallet_sk.sign_prehash(&resp_id_bytes).unwrap().to_bytes());

    let signed_resp_event = Event {
        id: resp_id,
        sig: resp_sig,
        ..resp_event
    };

    let parsed_resp = client
        .parse_response_event(&signed_resp_event, &request_id)
        .unwrap();
    assert_eq!(parsed_resp, resp_payload);
}

#[test]
fn test_nwc_client_nip44_roundtrip() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let mut client = NwcClient::new(nwc_uri).unwrap();
    client.encryption_method = EncryptionMethod::Nip44;

    let payload = serde_json::json!({"test": "nip44 data"});
    let encrypted = client.encrypt(&payload).unwrap();
    let decrypted = client.decrypt(&encrypted).unwrap();
    let decrypted_json: Value = serde_json::from_str(&decrypted).unwrap();
    assert_eq!(payload, decrypted_json);
}

#[test]
#[should_panic(expected = "InvalidScheme")]
fn test_uri_invalid_scheme() {
    let uri = "http://invalid.example.com?secret=0101010101010101010101010101010101010101010101010101010101010101";
    let _ = NwcUri::from_uri(uri).unwrap();
}

#[test]
fn test_uri_missing_pubkey_returns_error() {
    let uri = "nostr+walletconnect://?secret=0101010101010101010101010101010101010101010101010101010101010101";
    let result = NwcUri::from_uri(uri);
    assert!(result.is_err());
}

#[test]
fn test_uri_missing_secret_returns_error() {
    let uri =
        "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
    let result = NwcUri::from_uri(uri);
    assert!(result.is_err());
}

#[test]
fn test_client_encrypt_deterministic() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let client = NwcClient::new(nwc_uri).unwrap();

    let payload = serde_json::json!({"same": "data"});
    let encrypted1 = client.encrypt(&payload).unwrap();
    let encrypted2 = client.encrypt(&payload).unwrap();
    assert_ne!(encrypted1, encrypted2);
}

#[test]
fn test_client_created_has_required_fields() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let client = NwcClient::new(nwc_uri).unwrap();

    assert!(!client.my_pubkey.is_empty());
}

#[test]
fn test_kind_constants() {
    assert_eq!(KIND_NWC_REQUEST, 23194);
}

#[test]
fn test_encryption_method_from_protocol_str() {
    assert_eq!(
        EncryptionMethod::from_protocol_str("nip04"),
        Some(EncryptionMethod::Nip04)
    );
    assert_eq!(
        EncryptionMethod::from_protocol_str("nip44_v2"),
        Some(EncryptionMethod::Nip44)
    );
    assert_eq!(EncryptionMethod::from_protocol_str("unknown"), None);
}

#[test]
fn test_encryption_method_to_protocol_string() {
    assert_eq!(EncryptionMethod::Nip04.to_protocol_string(), "nip04");
    assert_eq!(EncryptionMethod::Nip44.to_protocol_string(), "nip44_v2");
}

#[test]
fn test_nwc_uri_error_display() {
    assert!(NwcUriError::InvalidUrl("bad".into())
        .to_string()
        .contains("url failure"));
    assert!(NwcUriError::InvalidScheme
        .to_string()
        .contains("Invalid scheme"));
    assert!(NwcUriError::MissingPubkey
        .to_string()
        .contains("Missing wallet pubkey"));
    assert!(NwcUriError::MissingSecret
        .to_string()
        .contains("Missing secret"));
}

#[test]
fn test_parse_wallet_info_no_encryption_tag_defaults_nip04() {
    let event = Event {
        id: "id1".into(),
        pubkey: "pk1".into(),
        created_at: 1000,
        kind: 13194,
        tags: vec![],
        content: String::new(),
        sig: "sig".into(),
    };
    let info = parse_wallet_info(&event);
    assert_eq!(info.encryption_algorithms, vec![EncryptionMethod::Nip04]);
}

fn wallet_signing_key() -> SigningKey {
    let sk_bytes = hex::decode(TEST_WALLET_SK).unwrap();
    let sk_arr: [u8; 32] = sk_bytes.try_into().unwrap();
    SigningKey::from_slice(&sk_arr).unwrap()
}

/// A second, deterministic keypair used to produce events that are correctly
/// signed but by the wrong identity.
fn other_signing_key() -> SigningKey {
    SigningKey::from_slice(&[2u8; 32]).unwrap()
}

fn signed_event(pubkey: &str, sk: &SigningKey, kind: u64, tags: Vec<Tag>, content: &str) -> Event {
    let created_at = crate::util::now();
    let (id, id_bytes) = Event::compute_id(pubkey, created_at, kind, &tags, content).unwrap();
    let sig = hex::encode(sk.sign_prehash(&id_bytes).unwrap().to_bytes());
    Event {
        id,
        pubkey: pubkey.into(),
        created_at,
        kind,
        tags,
        content: content.into(),
        sig,
    }
}

#[test]
fn test_nwc_client_new_rejects_non_hex_secret() {
    let uri = NwcUri {
        wallet_pubkey: TEST_WALLET_PK.into(),
        secret: "zz".into(),
    };
    let err = NwcClient::new(uri).err().expect("non-hex secret must fail");
    assert!(matches!(err, RelayError::MalformedHex(_)), "{err}");
}

#[test]
fn test_nwc_client_new_rejects_short_secret() {
    let uri = NwcUri {
        wallet_pubkey: TEST_WALLET_PK.into(),
        secret: "00".into(),
    };
    let err = NwcClient::new(uri).err().expect("short secret must fail");
    assert!(matches!(err, RelayError::CryptoError(_)), "{err}");
}

#[test]
fn test_nwc_client_new_rejects_non_hex_wallet_pubkey() {
    let uri = NwcUri {
        wallet_pubkey: "zz".into(),
        secret: TEST_WALLET_SK.into(),
    };
    let err = NwcClient::new(uri).err().expect("non-hex pubkey must fail");
    assert!(matches!(err, RelayError::MalformedHex(_)), "{err}");
}

#[test]
fn test_nwc_client_new_rejects_wrong_length_wallet_pubkey() {
    // A 31-byte key is valid hex but cannot be a secp256k1 x-only public key.
    let uri = NwcUri {
        wallet_pubkey: "11".repeat(31),
        secret: TEST_WALLET_SK.into(),
    };
    let err = NwcClient::new(uri).err().expect("short pubkey must fail");
    assert!(err.to_string().contains("32 bytes"), "{err}");
}

#[test]
fn test_nwc_client_new_rejects_invalid_wallet_pubkey_point() {
    // 32 bytes, but x = 0 is not on the curve.
    let uri = NwcUri {
        wallet_pubkey: "00".repeat(32),
        secret: TEST_WALLET_SK.into(),
    };
    let err = NwcClient::new(uri)
        .err()
        .expect("off-curve pubkey must fail");
    assert!(matches!(err, RelayError::CryptoError(_)), "{err}");
}

#[test]
fn test_parse_wallet_info_deduplicates_schemes() {
    let event = Event {
        id: "id1".into(),
        pubkey: "pk1".into(),
        created_at: 1000,
        kind: 13194,
        tags: vec![
            Tag::encryption("nip44_v2 nip44_v2 nip04"),
            Tag::encryption("nip04"),
        ],
        content: String::new(),
        sig: "sig".into(),
    };
    let info = parse_wallet_info(&event);
    assert_eq!(
        info.encryption_algorithms,
        vec![EncryptionMethod::Nip44, EncryptionMethod::Nip04]
    );
}

#[test]
fn test_parse_wallet_info_skips_unknown_schemes() {
    let event = Event {
        id: "id1".into(),
        pubkey: "pk1".into(),
        created_at: 1000,
        kind: 13194,
        tags: vec![Tag::encryption("nip04 future_scheme")],
        content: String::new(),
        sig: "sig".into(),
    };
    let info = parse_wallet_info(&event);
    assert_eq!(info.encryption_algorithms, vec![EncryptionMethod::Nip04]);
}

#[test]
fn test_create_request_event_carries_expiration_and_encryption() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let client = NwcClient::new(nwc_uri).unwrap();
    let (event, request_id) = client
        .create_request_event(NwcMethod::MakeInvoice, serde_json::json!({}), vec![])
        .unwrap();

    assert_eq!(event.id, request_id);
    assert!(event.tags.iter().any(|t| t.pubkey().is_some()));
    assert!(event.tags.iter().any(|t| matches!(t, Tag::Expiration(_))));
    assert_eq!(
        event.tags.iter().find_map(Tag::encryption_scheme),
        Some("nip04")
    );
}

#[test]
fn test_parse_response_event_rejects_pubkey_mismatch() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let client = NwcClient::new(nwc_uri).unwrap();
    let content = client.encrypt(&serde_json::json!({"result": {}})).unwrap();
    // Validly signed, but by an identity other than the wallet's.
    let other_sk = other_signing_key();
    let other_pk = hex::encode(other_sk.verifying_key().to_bytes());
    let event = signed_event(
        &other_pk,
        &other_sk,
        23194,
        vec![Tag::E("request_id".into(), vec![])],
        &content,
    );

    match client.parse_response_event(&event, "request_id") {
        Err(RelayError::Generic(msg)) => assert!(msg.contains("pubkey mismatch"), "{msg}"),
        other => panic!("expected Generic(pubkey mismatch), got {other:?}"),
    }
}

#[test]
fn test_parse_response_event_rejects_missing_e_tag() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let client = NwcClient::new(nwc_uri).unwrap();
    let content = client.encrypt(&serde_json::json!({"result": {}})).unwrap();
    // Kind 23197 only requires a `p` tag, so verification passes and the
    // missing `e` tag is the branch under test.
    let event = signed_event(
        &client.wallet_pubkey,
        &wallet_signing_key(),
        23197,
        vec![Tag::p(&client.my_pubkey)],
        &content,
    );

    match client.parse_response_event(&event, "request_id") {
        Err(RelayError::Generic(msg)) => assert!(msg.contains("missing 'e' tag"), "{msg}"),
        other => panic!("expected Generic(missing e tag), got {other:?}"),
    }
}

#[test]
fn test_parse_response_event_rejects_bad_signature() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let client = NwcClient::new(nwc_uri).unwrap();
    let content = client.encrypt(&serde_json::json!({"result": {}})).unwrap();
    // Correct pubkey and id, but the signature belongs to another key, so it
    // parses as a signature yet fails verification.
    let event = signed_event(
        &client.wallet_pubkey,
        &other_signing_key(),
        23194,
        vec![Tag::E("request_id".into(), vec![])],
        &content,
    );

    match client.parse_response_event(&event, "request_id") {
        Err(RelayError::InvalidSignature) => {}
        other => panic!("expected InvalidSignature, got {other:?}"),
    }
}

#[test]
fn test_parse_response_event_rejects_undecryptable_content() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let client = NwcClient::new(nwc_uri).unwrap();
    let event = signed_event(
        &client.wallet_pubkey,
        &wallet_signing_key(),
        23194,
        vec![Tag::E("request_id".into(), vec![])],
        "not-a-nip04-payload",
    );

    let err = client
        .parse_response_event(&event, "request_id")
        .expect_err("garbage content must not decrypt");
    assert!(matches!(err, RelayError::Generic(_)), "{err}");
}

#[test]
fn test_parse_response_event_rejects_non_json_payload() {
    let nwc_uri = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let client = NwcClient::new(nwc_uri).unwrap();
    // Encrypt a raw non-JSON body, which `NwcClient::encrypt` cannot produce
    // because it always serializes a `Value` first.
    let content =
        crate::nostr::nip_04::encrypt_nip04(&client.shared_secret, "not json at all").unwrap();
    let event = signed_event(
        &client.wallet_pubkey,
        &wallet_signing_key(),
        23194,
        vec![Tag::E("request_id".into(), vec![])],
        &content,
    );

    let err = client
        .parse_response_event(&event, "request_id")
        .expect_err("non-JSON body must be rejected");
    assert!(matches!(err, RelayError::SerializationError(_)), "{err}");
}
