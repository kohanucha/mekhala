pub mod nip_01;
pub mod nip_04;
pub mod nip_11;
pub mod nip_44;
pub mod nip_47;
pub mod engine;
pub mod state;
pub mod event;
pub mod filter;
pub mod limits;
pub mod error;

pub use nip_01::RelayMessage;
pub use nip_11::handle_get_info;
pub use event::Event;
pub use filter::Filter;
pub use limits::Limits;
pub use error::RelayError;

use crate::util::engine::Engine;
use crate::nostr::engine::NostrEngine;

pub fn create_engine() -> Box<dyn Engine> {
    Box::new(NostrEngine::new())
}

use k256::{PublicKey as K256PublicKey, SecretKey as K256SecretKey};
use worker::{Error, Result};

pub fn get_shared_secret(secret_key_hex: &str, public_key_hex: &str) -> Result<Vec<u8>> {
    let secret_key_bytes = hex::decode(secret_key_hex).map_err(|e| Error::from(e.to_string()))?;
    let sk =
        K256SecretKey::from_slice(&secret_key_bytes).map_err(|e| Error::from(e.to_string()))?;

    let public_key_bytes = hex::decode(public_key_hex).map_err(|e| Error::from(e.to_string()))?;
    let mut full_pk_bytes = [0u8; 33];
    full_pk_bytes[0] = 0x02;
    full_pk_bytes[1..].copy_from_slice(&public_key_bytes);

    let pk =
        K256PublicKey::from_sec1_bytes(&full_pk_bytes).map_err(|e| Error::from(e.to_string()))?;

    let shared = k256::ecdh::diffie_hellman(sk.to_nonzero_scalar(), pk.as_affine());
    Ok(shared.raw_secret_bytes().to_vec())
}
