use aes::Aes256;
use base64::{engine::general_purpose, Engine as _};
use block_padding::Pkcs7;
use cbc::{Decryptor, Encryptor};
use cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use rand::RngCore;
use crate::nostr::{RelayError, Result};

pub fn encrypt_nip04(shared_secret: &[u8], plaintext: &str) -> Result<String> {
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);

    let pt_bytes = plaintext.as_bytes();
    let mut buffer = vec![0u8; pt_bytes.len() + 16];
    buffer[..pt_bytes.len()].copy_from_slice(pt_bytes);

    let pos = pt_bytes.len();
    let ct = Encryptor::<Aes256>::new_from_slices(shared_secret, &iv)
        .map_err(|e| RelayError::CryptoError(e.to_string()))?
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, pos)
        .map_err(|e| RelayError::CryptoError(e.to_string()))?;

    let iv_b64 = general_purpose::STANDARD.encode(iv);
    let ct_b64 = general_purpose::STANDARD.encode(ct);

    Ok(format!("{}?iv={}", ct_b64, iv_b64))
}

pub fn decrypt_nip04(shared_secret: &[u8], encrypted_content: &str) -> Result<String> {
    let parts: Vec<&str> = encrypted_content.split("?iv=").collect();
    if parts.len() != 2 {
        return Err(RelayError::Generic("Invalid NIP-04 format".into()));
    }

    let mut ct_bytes = general_purpose::STANDARD
        .decode(parts[0])
        .map_err(|e| RelayError::Base64Error(e.to_string()))?;
    let iv_bytes = general_purpose::STANDARD
        .decode(parts[1])
        .map_err(|e| RelayError::Base64Error(e.to_string()))?;

    let pt = Decryptor::<Aes256>::new_from_slices(shared_secret, &iv_bytes)
        .map_err(|e| RelayError::CryptoError(e.to_string()))?
        .decrypt_padded_mut::<Pkcs7>(&mut ct_bytes)
        .map_err(|e| RelayError::CryptoError(e.to_string()))?;

    String::from_utf8(pt.to_vec()).map_err(|e| RelayError::Utf8Error(e.to_string()))
}

#[cfg(test)]
#[path = "nip_04_test.rs"]
mod nip_04_test;
