use super::*;

const URI: &str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=wss%3A%2F%2Frelay.example&secret=0101010101010101010101010101010101010101010101010101010101010101";

fn keys(current: Option<[u8; 32]>, previous: Option<[u8; 32]>) -> LnAddressKeys {
    LnAddressKeys { current, previous }
}

fn keys_with(byte: u8) -> LnAddressKeys {
    keys(Some([byte; 32]), None)
}

fn alice() -> LnAddressRecord {
    LnAddressRecord::new("alice", URI, None)
}

fn open(value: &str, username: &str, keys: &LnAddressKeys) -> Result<LnAddressRecord, RecordError> {
    open_sealed(value, username, keys)
}

#[test]
fn test_seal_then_open_round_trips() {
    let keys = keys_with(0x11);
    let sealed = seal(&alice(), &keys).unwrap();
    assert_eq!(open(&sealed, "alice", &keys).unwrap(), alice());
}

#[test]
fn test_round_trip_keeps_verified_methods() {
    let keys = keys_with(0x22);
    let methods = vec!["make_invoice".to_string(), "get_info".to_string()];
    let record = LnAddressRecord::new("alice", URI, Some(methods.clone()));
    let sealed = seal(&record, &keys).unwrap();
    assert_eq!(
        open(&sealed, "alice", &keys).unwrap().methods,
        Some(methods)
    );
}

#[test]
fn test_no_methods_are_omitted_from_the_payload() {
    let value = serde_json::to_value(alice()).unwrap();
    assert!(value.get("methods").is_none());
}

#[test]
fn test_sealed_value_carries_the_prefix() {
    let sealed = seal(&alice(), &keys_with(0x33)).unwrap();
    assert!(sealed.starts_with(SEALED_PREFIX));
}

#[test]
fn test_sealed_value_hides_the_uri_and_username() {
    let sealed = seal(&alice(), &keys_with(0x44)).unwrap();
    assert!(!sealed.contains("nostr+walletconnect"));
    assert!(!sealed.contains("alice"));
}

#[test]
fn test_sealing_twice_produces_different_values() {
    let keys = keys_with(0x55);
    let record = alice();
    assert_ne!(seal(&record, &keys).unwrap(), seal(&record, &keys).unwrap());
}

#[test]
fn test_open_refuses_another_username() {
    let keys = keys_with(0x66);
    let sealed = seal(&alice(), &keys).unwrap();
    assert_eq!(
        open(&sealed, "bob", &keys),
        Err(RecordError::UsernameMismatch)
    );
}

#[test]
fn test_open_with_the_previous_key_survives_rotation() {
    let old = keys_with(0x77);
    let sealed = seal(&alice(), &old).unwrap();

    let rotating = keys(Some([0x78; 32]), Some([0x77; 32]));
    assert_eq!(open(&sealed, "alice", &rotating).unwrap(), alice());
}

#[test]
fn test_open_fails_under_a_key_that_did_not_write_it() {
    let sealed = seal(&alice(), &keys_with(0x79)).unwrap();
    assert!(matches!(
        open(&sealed, "alice", &keys_with(0x7a)),
        Err(RecordError::Decrypt(_))
    ));
}

#[test]
fn test_seal_requires_a_key() {
    assert_eq!(
        seal(&alice(), &LnAddressKeys::default()),
        Err(RecordError::MissingKey)
    );
}

#[test]
fn test_open_requires_a_key() {
    let sealed = seal(&alice(), &keys_with(0x7b)).unwrap();
    assert_eq!(
        open(&sealed, "alice", &LnAddressKeys::default()),
        Err(RecordError::MissingKey)
    );
}

#[test]
fn test_open_rejects_a_value_without_the_prefix() {
    assert_eq!(
        open(URI, "alice", &keys_with(0x7c)),
        Err(RecordError::NotSealed)
    );
}

#[test]
fn test_open_rejects_a_tampered_payload() {
    let keys = keys_with(0x7d);
    let sealed = seal(&alice(), &keys).unwrap();
    let mut tampered = sealed.into_bytes();
    // Flip a character in the payload after the prefix.
    let index = SEALED_PREFIX.len() + 10;
    tampered[index] = if tampered[index] == b'A' { b'B' } else { b'A' };
    let tampered = String::from_utf8(tampered).unwrap();
    assert!(matches!(
        open(&tampered, "alice", &keys),
        Err(RecordError::Decrypt(_))
    ));
}

#[test]
fn test_open_rejects_a_truncated_payload() {
    let keys = keys_with(0x7e);
    let sealed = seal(&alice(), &keys).unwrap();
    let truncated = &sealed[..SEALED_PREFIX.len() + 12];
    assert!(matches!(
        open(truncated, "alice", &keys),
        Err(RecordError::Decrypt(_))
    ));
}

#[test]
fn test_open_rejects_a_newer_record_version() {
    let keys = keys_with(0x7f);
    let future = LnAddressRecord {
        version: RECORD_VERSION + 1,
        username: "alice".to_string(),
        nwc_uri: URI.to_string(),
        methods: None,
    };
    let sealed = seal(&future, &keys).unwrap();
    assert_eq!(
        open(&sealed, "alice", &keys),
        Err(RecordError::UnsupportedVersion)
    );
}

#[test]
fn test_open_rejects_a_payload_that_is_not_a_record() {
    let keys = keys_with(0x80);
    let key = derive_key(&[0x80; 32]).unwrap();
    let payload = encrypt_nip44(&key, "not a record").unwrap();
    let sealed = format!("{SEALED_PREFIX}{payload}");
    assert_eq!(
        open(&sealed, "alice", &keys),
        Err(RecordError::InvalidPayload)
    );
}

#[test]
fn test_open_rejects_a_record_missing_fields() {
    let keys = keys_with(0x81);
    let key = derive_key(&[0x81; 32]).unwrap();
    let payload = encrypt_nip44(&key, r#"{"v":1,"u":"alice"}"#).unwrap();
    let sealed = format!("{SEALED_PREFIX}{payload}");
    assert_eq!(
        open(&sealed, "alice", &keys),
        Err(RecordError::InvalidPayload)
    );
}

#[test]
fn test_derive_key_is_deterministic_and_secret_dependent() {
    assert_eq!(derive_key(&[1; 32]).unwrap(), derive_key(&[1; 32]).unwrap());
    assert_ne!(derive_key(&[1; 32]).unwrap(), derive_key(&[2; 32]).unwrap());
}

#[test]
fn test_key_for_prefixes_the_username() {
    assert_eq!(key_for("alice"), "lnaddr:alice");
    assert!(key_for("x").starts_with(KEY_PREFIX));
}

#[test]
fn test_from_hex_accepts_64_hex_characters() {
    let keys = LnAddressKeys::from_hex(Some("00".repeat(32)), None);
    assert!(keys.is_configured());
}

#[test]
fn test_from_hex_accepts_uppercase_and_whitespace() {
    let keys = LnAddressKeys::from_hex(Some(format!("  {}  ", "AB".repeat(32))), None);
    assert!(keys.is_configured());
}

#[test]
fn test_from_hex_rejects_a_short_key() {
    let keys = LnAddressKeys::from_hex(Some("ab".repeat(31)), None);
    assert!(!keys.is_configured());
}

#[test]
fn test_from_hex_rejects_a_long_key() {
    let keys = LnAddressKeys::from_hex(Some("ab".repeat(33)), None);
    assert!(!keys.is_configured());
}

#[test]
fn test_from_hex_rejects_non_hex_characters() {
    let keys = LnAddressKeys::from_hex(Some("zz".repeat(32)), None);
    assert!(!keys.is_configured());
}

#[test]
fn test_from_hex_treats_empty_as_missing() {
    assert!(!LnAddressKeys::from_hex(Some(String::new()), None).is_configured());
    assert!(!LnAddressKeys::from_hex(Some("   ".into()), None).is_configured());
    assert!(!LnAddressKeys::from_hex(None, None).is_configured());
}

#[test]
fn test_from_hex_keeps_the_previous_key() {
    let keys = LnAddressKeys::from_hex(Some("01".repeat(32)), Some("02".repeat(32)));
    let record = alice();
    let key = derive_key(&[2; 32]).unwrap();
    let payload = encrypt_nip44(&key, &serde_json::to_string(&record).unwrap()).unwrap();
    let sealed = format!("{SEALED_PREFIX}{payload}");
    assert_eq!(open(&sealed, "alice", &keys).unwrap(), record);
}

#[test]
fn test_from_hex_drops_a_malformed_previous_key() {
    let keys = LnAddressKeys::from_hex(Some("01".repeat(32)), Some("zz".repeat(32)));
    assert!(keys.is_configured());

    let key = derive_key(&[2; 32]).unwrap();
    let payload = encrypt_nip44(&key, &serde_json::to_string(&alice()).unwrap()).unwrap();
    let sealed = format!("{SEALED_PREFIX}{payload}");
    assert!(matches!(
        open(&sealed, "alice", &keys),
        Err(RecordError::Decrypt(_))
    ));
}

#[test]
fn test_from_hex_with_only_a_previous_key_cannot_write() {
    let keys = LnAddressKeys::from_hex(None, Some("02".repeat(32)));
    assert!(!keys.is_configured());
    assert_eq!(seal(&alice(), &keys), Err(RecordError::MissingKey));
}

#[test]
fn test_debug_hides_the_uri_and_key_material() {
    let record = alice();
    let debug = format!("{record:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("nostr+walletconnect"));

    let keys = keys(Some([0xab; 32]), Some([0xcd; 32]));
    let debug = format!("{keys:?}");
    assert!(debug.contains("current: true"));
    assert!(debug.contains("previous: true"));
    assert!(!debug.contains("abababab"));
}

#[test]
fn test_error_messages_do_not_include_secrets() {
    let messages = [
        RecordError::MissingKey.to_string(),
        RecordError::NotSealed.to_string(),
        RecordError::UnsupportedVersion.to_string(),
        RecordError::InvalidPayload.to_string(),
        RecordError::UsernameMismatch.to_string(),
        RecordError::KeyDerivation.to_string(),
    ];
    for message in messages {
        assert!(!message.contains("nostr+walletconnect"));
        assert!(!message.is_empty());
    }
}
