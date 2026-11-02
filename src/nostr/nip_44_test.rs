use super::*;

#[test]
fn test_nip44_padding() {
    let p1 = "short";
    let padded1 = pad(p1);
    assert_eq!(padded1.len(), 2 + 32);

    let p2 = "a".repeat(33);
    let padded2 = pad(&p2);
    assert_eq!(padded2.len(), 2 + 64);

    let unpadded1 = unpad(&padded1).unwrap();
    assert_eq!(unpadded1, p1);

    let unpadded2 = unpad(&padded2).unwrap();
    assert_eq!(unpadded2, p2);
}

#[test]
fn test_nip44_roundtrip() {
    let shared_secret = [42u8; 32];
    let plaintext = "Hello NIP-44 v2!";
    let encrypted = encrypt_nip44(&shared_secret, plaintext).unwrap();
    let decrypted = decrypt_nip44(&shared_secret, &encrypted).unwrap();
    assert_eq!(plaintext, decrypted);
}

#[test]
fn test_nip44_empty_message() {
    let shared_secret = [1u8; 32];
    let plaintext = "";
    let encrypted = encrypt_nip44(&shared_secret, plaintext).unwrap();
    let decrypted = decrypt_nip44(&shared_secret, &encrypted).unwrap();
    assert_eq!(plaintext, decrypted);
}

#[test]
fn test_nip44_extensive_padding() {
    let shared_secret = [1u8; 32];
    // Test various lengths to trigger different power-of-two chunks
    let lengths = [1, 31, 32, 33, 127, 128, 129, 255, 256, 257, 511, 512, 513];
    for &len in &lengths {
        let plaintext = "a".repeat(len);
        let encrypted = encrypt_nip44(&shared_secret, &plaintext).unwrap();
        let decrypted = decrypt_nip44(&shared_secret, &encrypted).unwrap();
        assert_eq!(plaintext, decrypted, "Failed at length {len}");
    }
}

#[test]
fn test_nip44_mac_failure() {
    let shared_secret = vec![1u8; 32];
    let plaintext = "secret message";
    let encrypted = encrypt_nip44(&shared_secret, plaintext).unwrap();

    // Tamper with the ciphertext (last 32 bytes are MAC, so tamper before that)
    let mut bytes = general_purpose::STANDARD.decode(&encrypted).unwrap();
    let len = bytes.len();
    bytes[len - 33] ^= 0xff; // Flip a bit in ciphertext
    let tampered_encrypted = general_purpose::STANDARD.encode(bytes);

    let result = decrypt_nip44(&shared_secret, &tampered_encrypted);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "error: crypto failure: Invalid NIP-44 MAC"
    );
}

#[test]
fn test_nip44_unsupported_version() {
    let shared_secret = [1u8; 32];
    let mut payload = vec![0x03]; // Version 3
    payload.resize(65, 0);
    let encrypted = general_purpose::STANDARD.encode(payload);
    let result = decrypt_nip44(&shared_secret, &encrypted);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "error: Unsupported NIP-44 version"
    );
}

#[test]
fn test_nip44_invalid_length() {
    let shared_secret = [1u8; 32];
    let mut payload = vec![0x02]; // Too short
    payload.resize(10, 0);
    let encrypted = general_purpose::STANDARD.encode(payload);
    let result = decrypt_nip44(&shared_secret, &encrypted);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "error: Invalid NIP-44 payload length"
    );
}

#[test]
fn test_nip44_wrong_key() {
    let key1 = [1u8; 32];
    let key2 = [2u8; 32];
    let plaintext = "Super secret";
    let encrypted = encrypt_nip44(&key1, plaintext).unwrap();
    let result = decrypt_nip44(&key2, &encrypted);
    assert!(result.is_err());
    // Should fail MAC verification
    assert_eq!(
        result.unwrap_err().to_string(),
        "error: crypto failure: Invalid NIP-44 MAC"
    );
}

#[test]
fn test_nip44_payload_too_large() {
    let shared_secret = [1u8; 32];
    let oversized = "a".repeat(87473);
    let result = decrypt_nip44(&shared_secret, &oversized);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "error: NIP-44 payload too large"
    );
}

#[test]
fn test_nip44_decoded_too_large() {
    let shared_secret = [1u8; 32];
    let payload = vec![0x02u8; 65604];
    // Fill content beyond size limit
    let encrypted = base64::engine::general_purpose::STANDARD.encode(&payload);
    let result = decrypt_nip44(&shared_secret, &encrypted);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "error: NIP-44 decoded payload too large"
    );
}

#[test]
fn test_nip44_unpad_too_short() {
    let result = crate::nostr::nip_44::unpad(&[]);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "error: Invalid padding");

    let result = crate::nostr::nip_44::unpad(&[0x00]);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "error: Invalid padding");
}

#[test]
fn test_nip44_unpad_invalid_length() {
    let padded = vec![0x00, 0x05, 0x01, 0x02]; // claims 5 bytes but only 2 available
    let result = crate::nostr::nip_44::unpad(&padded);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "error: Invalid padding length"
    );
}

#[test]
fn test_nip44_non_utf8_plaintext() {
    // Build a well-formed payload (valid MAC, valid padding) whose content is
    // not UTF-8: decryption must report Utf8Error rather than panicking.
    let shared_secret = [42u8; 32];
    let nonce = [9u8; 32];
    let conversation_key = derive_conversation_key(&shared_secret);
    let (chacha_key, chacha_nonce, hmac_key) =
        derive_message_keys(&conversation_key, &nonce).unwrap();

    let mut padded = vec![0u8; 32];
    padded[0] = 0x00;
    padded[1] = 0x03;
    padded[2..5].copy_from_slice(&[0xff, 0xfe, 0xfd]);

    let mut ciphertext = padded;
    let mut cipher = ChaCha20::new_from_slices(&chacha_key, &chacha_nonce).unwrap();
    cipher.apply_keystream(&mut ciphertext);

    let mut hmac = Hmac::<Sha256>::new_from_slice(&hmac_key).unwrap();
    hmac.update(&nonce);
    hmac.update(&ciphertext);
    let mac = hmac.finalize().into_bytes();

    let mut payload = vec![0x02];
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);
    payload.extend_from_slice(&mac);
    let encrypted = general_purpose::STANDARD.encode(&payload);

    match decrypt_nip44(&shared_secret, &encrypted) {
        Err(RelayError::Utf8Error(msg)) => assert!(!msg.is_empty()),
        other => panic!("expected Utf8Error, got {other:?}"),
    }
}

#[test]
fn test_nip44_invalid_base64() {
    let result = decrypt_nip44(&[1u8; 32], "not base64!!");
    assert!(
        matches!(result, Err(RelayError::Base64Error(_))),
        "{result:?}"
    );
}

#[test]
fn test_nip44_pad_lengths_follow_the_v2_buckets() {
    // Bucket edges: 32-byte steps up to 256 bytes, then chunked at 1/8 of the
    // next power of two.
    let cases = [
        (0usize, 32usize),
        (1, 32),
        (32, 32),
        (33, 64),
        (64, 64),
        (65, 96),
        (96, 96),
        (97, 128),
        (128, 128),
        (129, 160),
        (160, 160),
        (161, 192),
        (256, 256),
        (257, 320),
        (320, 320),
        (321, 384),
        (511, 512),
        (512, 512),
        (513, 640),
    ];
    for (len, expected) in cases {
        let plaintext = "a".repeat(len);
        let padded = pad(&plaintext);
        assert_eq!(padded.len(), 2 + expected, "plaintext length {len}");
        assert_eq!(&padded[..2], &(len as u16).to_be_bytes());
        assert_eq!(unpad(&padded).unwrap(), plaintext);
    }
}

#[test]
fn test_nip44_unpad_minimum_length() {
    // A two-byte payload is a valid empty message; it must not be rejected.
    assert_eq!(unpad(&[0, 0]).unwrap(), "");
    assert_eq!(unpad(&[0, 1, b'a']).unwrap(), "a");
}

#[test]
fn test_nip44_size_boundaries() {
    let shared_secret = [1u8; 32];
    // Smallest acceptable payload: version + 32-byte nonce + 32-byte MAC.
    // It must reach the MAC check rather than be rejected as too short.
    let min = general_purpose::STANDARD.encode(vec![0x02u8; 65]);
    let err =
        decrypt_nip44(&shared_secret, &min).expect_err("empty ciphertext cannot authenticate");
    assert_eq!(err.to_string(), "error: crypto failure: Invalid NIP-44 MAC");

    // Exactly at the decoded ceiling (65603 bytes) the payload is still
    // processed; only one byte more is rejected.
    let max = general_purpose::STANDARD.encode(vec![0x02u8; 65603]);
    let err = decrypt_nip44(&shared_secret, &max).expect_err("random MAC cannot match");
    assert_eq!(err.to_string(), "error: crypto failure: Invalid NIP-44 MAC");
}
