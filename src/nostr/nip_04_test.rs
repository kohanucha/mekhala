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
        assert_eq!(result.unwrap_err().to_string(), "error: Invalid NIP-04 format");
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
        let tampered_ct = &parts[0][..parts[0].len()-1];
        let tampered_encrypted = format!("{}?iv={}", tampered_ct, parts[1]);
        
        let result = decrypt_nip04(&shared_secret, &tampered_encrypted);
        // This will now consistently fail due to malformed base64 or invalid padding
        assert!(result.is_err());
    }
