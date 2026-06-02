use base64::{engine::general_purpose, Engine as _};
use chacha20::cipher::StreamCipher;
use chacha20::ChaCha20;
use chacha20::cipher::KeyIvInit as _;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Sha256};
use crate::nostr::{RelayError, Result};

pub fn encrypt_nip44(shared_secret: &[u8], plaintext: &str) -> Result<String> {
    let conversation_key = derive_conversation_key(shared_secret);
    let mut nonce = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut nonce);

    let (chacha_key, chacha_nonce, hmac_key) = derive_message_keys(&conversation_key, &nonce);

    let padded = pad(plaintext);
    let mut ciphertext = padded.clone();
    let mut cipher = ChaCha20::new_from_slices(&chacha_key, &chacha_nonce)
        .map_err(|e| RelayError::CryptoError(e.to_string()))?;
    cipher.apply_keystream(&mut ciphertext);

    let mut hmac = Hmac::<Sha256>::new_from_slice(&hmac_key)
        .map_err(|e| RelayError::CryptoError(e.to_string()))?;
    hmac.update(&nonce);
    hmac.update(&ciphertext);
    let mac = hmac.finalize().into_bytes();

    let mut payload = Vec::with_capacity(1 + 32 + ciphertext.len() + 32);
    payload.push(0x02);
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);
    payload.extend_from_slice(&mac);

    Ok(general_purpose::STANDARD.encode(payload))
}

pub fn decrypt_nip44(shared_secret: &[u8], encrypted_content: &str) -> Result<String> {
    if encrypted_content.len() > 87472 {
        return Err(RelayError::Generic("NIP-44 payload too large".into()));
    }

    let payload = general_purpose::STANDARD
        .decode(encrypted_content)
        .map_err(|e| RelayError::Base64Error(e.to_string()))?;

    if payload.len() > 65603 {
        return Err(RelayError::Generic("NIP-44 decoded payload too large".into()));
    }

    if payload.is_empty() || payload[0] != 0x02 {
        return Err(RelayError::Generic("Unsupported NIP-44 version".into()));
    }

    if payload.len() < 1 + 32 + 32 {
        return Err(RelayError::Generic("Invalid NIP-44 payload length".into()));
    }

    let nonce = &payload[1..33];
    let mac_start = payload.len() - 32;
    let ciphertext = &payload[33..mac_start];
    let mac = &payload[mac_start..];

    let conversation_key = derive_conversation_key(shared_secret);
    let (chacha_key, chacha_nonce, hmac_key) = derive_message_keys(&conversation_key, nonce);

    let mut hmac = Hmac::<Sha256>::new_from_slice(&hmac_key)
        .map_err(|e| RelayError::CryptoError(e.to_string()))?;
    hmac.update(nonce);
    hmac.update(ciphertext);
    if hmac.verify_slice(mac).is_err() {
        return Err(RelayError::CryptoError("Invalid NIP-44 MAC".into()));
    }

    let mut plaintext = ciphertext.to_vec();
    let mut cipher = ChaCha20::new_from_slices(&chacha_key, &chacha_nonce)
        .map_err(|e| RelayError::CryptoError(e.to_string()))?;
    cipher.apply_keystream(&mut plaintext);

    unpad(&plaintext)
}

fn derive_conversation_key(shared_secret: &[u8]) -> [u8; 32] {
    let (prk, _) = Hkdf::<Sha256>::extract(Some(b"nip44-v2"), shared_secret);
    let mut key = [0u8; 32];
    key.copy_from_slice(&prk);
    key
}

fn derive_message_keys(conversation_key: &[u8; 32], nonce: &[u8]) -> ([u8; 32], [u8; 12], [u8; 32]) {
    let hkdf = Hkdf::<Sha256>::from_prk(conversation_key).expect("HKDF from_prk failed");
    let mut okm = [0u8; 76]; // 32 + 12 + 32
    hkdf.expand(nonce, &mut okm).expect("HKDF expand failed");

    let mut chacha_key = [0u8; 32];
    let mut chacha_nonce = [0u8; 12];
    let mut hmac_key = [0u8; 32];

    chacha_key.copy_from_slice(&okm[0..32]);
    chacha_nonce.copy_from_slice(&okm[32..44]);
    hmac_key.copy_from_slice(&okm[44..76]);

    (chacha_key, chacha_nonce, hmac_key)
}

pub fn pad(plaintext: &str) -> Vec<u8> {
    let bytes = plaintext.as_bytes();
    let unpadded_len = bytes.len();
    
    let padded_len = if unpadded_len <= 32 {
        32
    } else {
        let next_power = 1 << (((unpadded_len - 1) as f64).log2().floor() as u32 + 1);
        let chunk = if next_power <= 256 {
            32
        } else {
            next_power / 8
        };
        chunk * ((unpadded_len - 1) / chunk + 1)
    };

    let mut padded = Vec::with_capacity(2 + padded_len);
    padded.extend_from_slice(&(unpadded_len as u16).to_be_bytes());
    padded.extend_from_slice(bytes);
    padded.resize(2 + padded_len, 0);
    padded
}

pub fn unpad(padded: &[u8]) -> Result<String> {
    if padded.len() < 2 {
        return Err(RelayError::Generic("Invalid padding".into()));
    }
    let len = u16::from_be_bytes([padded[0], padded[1]]) as usize;
    if len + 2 > padded.len() {
        return Err(RelayError::Generic("Invalid padding length".into()));
    }
    String::from_utf8(padded[2..2 + len].to_vec()).map_err(|e| RelayError::Utf8Error(e.to_string()))
}

#[cfg(test)]
#[path = "nip_44_test.rs"]
mod nip_44_test;
