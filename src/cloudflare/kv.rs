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
use std::sync::{Mutex, OnceLock};

/// Usernames whose plaintext row this isolate has already looked for.
///
/// A read that finds a sealed record still checks once for a plaintext row,
/// because a migration whose delete failed leaves both behind and the next read
/// would otherwise never retry. The set is bounded: past the limit the check
/// simply runs on every request again, which costs a read but keeps memory flat
/// and still cleans the row up.
const LEGACY_CHECK_LIMIT: usize = 256;

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
                Ok(record) => {
                    // The sealed row wins over any plaintext row, but a
                    // leftover must not stay at rest: a migration whose delete
                    // failed leaves both behind, and this is the only place
                    // that can notice.
                    self.clear_legacy_row(username).await;
                    Some(record)
                }
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
        self.delete_legacy_row(username).await;
        Some(record)
    }

    /// Deletes a plaintext row for `username` if one is still present, and
    /// remembers that the check has been made in this isolate.
    async fn clear_legacy_row(&self, username: &str) {
        if !legacy_check_pending(username) {
            return;
        }
        match self.kv.get(username).text().await {
            Ok(None) => mark_legacy_checked(username),
            Ok(Some(_)) => self.delete_legacy_row(username).await,
            Err(e) => {
                log_warn!(
                    "could not check the legacy record for '{}': {}",
                    username,
                    e
                );
            }
        }
    }

    /// Deletes `username`'s plaintext row, remembering the check once it
    /// succeeds so a failure is retried on the next read.
    async fn delete_legacy_row(&self, username: &str) {
        match self.kv.delete(username).await {
            Ok(()) => mark_legacy_checked(username),
            Err(e) => {
                log_warn!(
                    "plaintext record for '{}' left behind after migration: {}",
                    username,
                    e
                );
            }
        }
    }
}

fn legacy_checked() -> &'static Mutex<Vec<String>> {
    static CHECKED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    CHECKED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Whether the plaintext row for `username` still needs a look in this isolate.
fn legacy_check_pending(username: &str) -> bool {
    match legacy_checked().lock() {
        Ok(checked) => !checked.iter().any(|name| name == username),
        // A poisoned lock costs one repeated check; never skip the cleanup
        // because of it.
        Err(_) => true,
    }
}

/// Remembers that `username` has no plaintext row left to clear.
fn mark_legacy_checked(username: &str) {
    if let Ok(mut checked) = legacy_checked().lock() {
        if checked.len() < LEGACY_CHECK_LIMIT {
            checked.push(username.to_owned());
        }
    }
}

#[async_trait::async_trait(?Send)]
impl UserStore for CloudflareKvStore {
    async fn get_nwc_uri(&self, username: &str) -> Option<String> {
        self.get_record(username).await.map(|record| record.nwc_uri)
    }
}
