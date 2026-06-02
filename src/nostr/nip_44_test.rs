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
            assert_eq!(plaintext, decrypted, "Failed at length {}", len);
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
        assert_eq!(result.unwrap_err().to_string(), "error: crypto failure: Invalid NIP-44 MAC");
    }

    #[test]
    fn test_nip44_unsupported_version() {
        let shared_secret = [1u8; 32];
        let mut payload = vec![0x03]; // Version 3
        payload.resize(65, 0);
        let encrypted = general_purpose::STANDARD.encode(payload);
        let result = decrypt_nip44(&shared_secret, &encrypted);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "error: Unsupported NIP-44 version");
    }

    #[test]
    fn test_nip44_invalid_length() {
        let shared_secret = [1u8; 32];
        let mut payload = vec![0x02]; // Too short
        payload.resize(10, 0);
        let encrypted = general_purpose::STANDARD.encode(payload);
        let result = decrypt_nip44(&shared_secret, &encrypted);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "error: Invalid NIP-44 payload length");
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
        assert_eq!(result.unwrap_err().to_string(), "error: crypto failure: Invalid NIP-44 MAC");
    }

    #[test]
    fn test_nip44_payload_too_large() {
        let shared_secret = [1u8; 32];
        let oversized = "a".repeat(87473);
        let result = decrypt_nip44(&shared_secret, &oversized);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "error: NIP-44 payload too large");
    }

    #[test]
    fn test_nip44_decoded_too_large() {
        let shared_secret = [1u8; 32];
        let payload = vec![0x02u8; 65604];
        // Fill content beyond size limit
        let encrypted = base64::engine::general_purpose::STANDARD.encode(&payload);
        let result = decrypt_nip44(&shared_secret, &encrypted);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "error: NIP-44 decoded payload too large");
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
        assert_eq!(result.unwrap_err().to_string(), "error: Invalid padding length");
    }
