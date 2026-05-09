use aes::Aes256;
use base64::{engine::general_purpose, Engine as _};
use block_padding::Pkcs7;
use cbc::{Decryptor, Encryptor};
use cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use rand::RngCore;
use worker::{Error, Result};

pub fn encrypt_nip04(shared_secret: &[u8], plaintext: &str) -> Result<String> {
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);

    let pt_bytes = plaintext.as_bytes();
    let mut buffer = vec![0u8; pt_bytes.len() + 16];
    buffer[..pt_bytes.len()].copy_from_slice(pt_bytes);

    let pos = pt_bytes.len();
    let ct = Encryptor::<Aes256>::new_from_slices(shared_secret, &iv)
        .map_err(|e| Error::from(e.to_string()))?
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, pos)
        .map_err(|e| Error::from(e.to_string()))?;

    let iv_b64 = general_purpose::STANDARD.encode(iv);
    let ct_b64 = general_purpose::STANDARD.encode(ct);

    Ok(format!("{}?iv={}", ct_b64, iv_b64))
}

pub fn decrypt_nip04(shared_secret: &[u8], encrypted_content: &str) -> Result<String> {
    let parts: Vec<&str> = encrypted_content.split("?iv=").collect();
    if parts.len() != 2 {
        return Err(Error::from("Invalid NIP-04 format"));
    }

    let mut ct_bytes = general_purpose::STANDARD
        .decode(parts[0])
        .map_err(|e| Error::from(e.to_string()))?;
    let iv_bytes = general_purpose::STANDARD
        .decode(parts[1])
        .map_err(|e| Error::from(e.to_string()))?;

    let pt = Decryptor::<Aes256>::new_from_slices(shared_secret, &iv_bytes)
        .map_err(|e| Error::from(e.to_string()))?
        .decrypt_padded_mut::<Pkcs7>(&mut ct_bytes)
        .map_err(|e| Error::from(e.to_string()))?;

    String::from_utf8(pt.to_vec()).map_err(|e| Error::from(e.to_string()))
}

#[cfg(test)]
mod tests {
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
        assert_eq!(result.unwrap_err().to_string(), "Invalid NIP-04 format");
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
}
