//! Fixtures shared by this crate's unit tests.
//!
//! The relay crate keeps its own copies in `src/common/test_helpers.rs`:
//! `cfg(test)` items are not visible across crates.

use crate::{Event, Tag};

pub const TEST_WALLET_SK: &str = "0101010101010101010101010101010101010101010101010101010101010101";
pub const TEST_WALLET_PK: &str = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
pub const TEST_NWC_URI: &str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";

pub fn test_event(id: &str, pubkey: &str, kind: u64, tags: Vec<Tag>, created_at: u64) -> Event {
    Event {
        id: id.into(),
        pubkey: pubkey.into(),
        kind,
        tags,
        content: "test".into(),
        sig: "sig".into(),
        created_at,
    }
}
