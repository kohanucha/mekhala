//! KV-backed storage for Lightning Address registrations.
//!
//! Records written by this version live under `lnaddr:<username>` as sealed
//! payloads (see [`crate::lnaddress::record`]). Earlier versions stored the NWC
//! URI as plaintext under the bare username; those rows are moved to the sealed
//! layout the first time they are read, so no manual migration is needed.
//!
//! Without `LN_ADDRESS_KEY` nothing is served or stored: reading a sealed
//! record cannot work, and falling back to plaintext would silently downgrade
//! the guarantee the sealed layout exists for.

use crate::common::UserStore;
use crate::lnaddress::record::{self, LnAddressKeys, LnAddressRecord};
use crate::log_error;
use crate::log_warn;

pub struct CloudflareKvStore {
    kv: worker::KvStore,
    keys: LnAddressKeys,
}

impl CloudflareKvStore {
    pub fn new(kv: worker::KvStore, keys: LnAddressKeys) -> Self {
        Self { kv, keys }
    }

    /// Reads the registration for `username`, migrating a plaintext row from
    /// an earlier version on the way.
    ///
    /// Returns `None` for a missing row, for a sealed row this build cannot
    /// open, and for a plaintext row when no key is configured.
    pub async fn get_record(&self, username: &str) -> Option<LnAddressRecord> {
        let key = record::key_for(username);
        let stored = self.kv.get(&key).text().await.ok().flatten();
        if let Some(value) = stored {
            return match record::open_sealed(&value, username, &self.keys) {
                Ok(record) => Some(record),
                Err(e) => {
                    log_warn!("LN address record for '{}' is unreadable: {}", username, e);
                    None
                }
            };
        }
        self.migrate_legacy(username).await
    }

    /// Moves a plaintext row written by an earlier version to the sealed
    /// layout and returns it.
    ///
    /// The sealed row is written before the plaintext one is deleted, so an
    /// interruption leaves the old row in place rather than losing the URI.
    async fn migrate_legacy(&self, username: &str) -> Option<LnAddressRecord> {
        let legacy = self.kv.get(username).text().await.ok().flatten()?;
        if !self.keys.is_configured() {
            log_error!(
                "LN_ADDRESS_KEY is not configured; refusing the plaintext record for '{}'",
                username
            );
            return None;
        }

        let record = LnAddressRecord::new(username, &legacy, None);
        let sealed = match record::seal(&record, &self.keys) {
            Ok(sealed) => sealed,
            Err(e) => {
                log_error!("could not seal the record for '{}': {}", username, e);
                return None;
            }
        };

        let stored = match self.kv.put(&record::key_for(username), sealed) {
            Ok(builder) => builder.execute().await,
            Err(e) => Err(e),
        };
        if let Err(e) = stored {
            log_error!(
                "could not store the sealed record for '{}': {}",
                username,
                e
            );
            return Some(record);
        }
        if let Err(e) = self.kv.delete(username).await {
            log_warn!(
                "plaintext record for '{}' left behind after migration: {}",
                username,
                e
            );
        }
        Some(record)
    }
}

#[async_trait::async_trait(?Send)]
impl UserStore for CloudflareKvStore {
    async fn get_nwc_uri(&self, username: &str) -> Option<String> {
        self.get_record(username).await.map(|record| record.nwc_uri)
    }
}
