use super::*;

#[test]
fn test_nip04_roundtrip() {
    let shared_secret = [42u8; 32];
    let plaintext = "Hello, Nostr!";

    let encrypted = encrypt_nip04(&shared_secret, plaintext).unwrap();
    assert!(encrypted.contains("?iv="));

    let decrypted = decrypt_nip04(&shared_secret, &encrypted).unwrap();
    assert_eq!(plaintext, decrypted);
}

#[test]
fn test_nip04_invalid_format() {
    let shared_secret = [42u8; 32];
    let result = decrypt_nip04(&shared_secret, "not-base64-content");
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "error: Invalid NIP-04 format"
    );
}

#[test]
fn test_nip04_wrong_key() {
    let shared_secret1 = [1u8; 32];
    let shared_secret2 = [2u8; 32];
    let plaintext = "Sensitive data";

    let encrypted = encrypt_nip04(&shared_secret1, plaintext).unwrap();
    let result = decrypt_nip04(&shared_secret2, &encrypted);

    // Either it fails to unpad or returns garbage
    assert!(result.is_err());
}

#[test]
fn test_nip04_tampered_ciphertext() {
    let shared_secret = [42u8; 32];
    let plaintext = "Another secret";

    let encrypted = encrypt_nip04(&shared_secret, plaintext).unwrap();
    // Tamper by removing the last character of the ciphertext to break base64/padding
    let parts: Vec<&str> = encrypted.split("?iv=").collect();
    let tampered_ct = &parts[0][..parts[0].len() - 1];
    let tampered_encrypted = format!("{}?iv={}", tampered_ct, parts[1]);

    let result = decrypt_nip04(&shared_secret, &tampered_encrypted);
    // This will now consistently fail due to malformed base64 or invalid padding
    assert!(result.is_err());
}

#[test]
fn test_nip04_invalid_base64_iv() {
    let shared_secret = [42u8; 32];
    let result = decrypt_nip04(&shared_secret, "AAAA?iv=!!!");
    match result {
        Err(RelayError::Base64Error(msg)) => assert!(!msg.is_empty()),
        other => panic!("expected Base64Error, got {other:?}"),
    }
}

#[test]
fn test_nip04_iv_wrong_length() {
    let shared_secret = [42u8; 32];
    let short_iv = general_purpose::STANDARD.encode([0u8; 8]);
    let result = decrypt_nip04(&shared_secret, &format!("AAAA?iv={short_iv}"));
    match result {
        Err(RelayError::CryptoError(msg)) => assert!(!msg.is_empty()),
        other => panic!("expected CryptoError, got {other:?}"),
    }
}

#[test]
fn test_nip04_non_utf8_plaintext() {
    let shared_secret = [42u8; 32];
    let iv = [7u8; 16];
    // 0xFF is never valid UTF-8, so decryption succeeds but the final
    // String conversion must be rejected instead of panicking.
    let plaintext: [u8; 3] = [0xff, 0xfe, 0xfd];
    let mut buffer = vec![0u8; plaintext.len() + 16];
    buffer[..plaintext.len()].copy_from_slice(&plaintext);

    let ct = Encryptor::<Aes256>::new_from_slices(&shared_secret, &iv)
        .unwrap()
        .encrypt_padded::<Pkcs7>(&mut buffer, plaintext.len())
        .unwrap();
    let payload = format!(
        "{}?iv={}",
        general_purpose::STANDARD.encode(ct),
        general_purpose::STANDARD.encode(iv)
    );

    match decrypt_nip04(&shared_secret, &payload) {
        Err(RelayError::Utf8Error(msg)) => assert!(!msg.is_empty()),
        other => panic!("expected Utf8Error, got {other:?}"),
    }
}
