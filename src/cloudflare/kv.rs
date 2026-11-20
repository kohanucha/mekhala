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

/// Keys fetched per page. Every key costs one more KV read for its value, and
/// a Worker request may make a limited number of external calls, so one request
/// reads one page and hands the caller a cursor for the next.
const LIST_PAGE_SIZE: u64 = 100;

/// Why a page of registrations could not be read.
///
/// A failed listing is never reported as an empty one: an admin who sees
/// "nothing stored" would stop looking, and a rotation that reports "0 scanned"
/// after a listing failure could be followed by dropping the previous key.
const LIST_FAILED: &str = "could not list registrations";

/// One page of `lnaddr:` keys, with the cursor that follows it.
struct KeyPage {
    names: Vec<String>,
    /// Cursor for the next page, or `None` when the namespace is exhausted.
    next_cursor: Option<String>,
}

/// One page of registrations, with the cursor that follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordPage {
    /// Registrations this build could open.
    pub records: Vec<LnAddressRecord>,
    /// Cursor for the next page, or `None` at the end of the namespace.
    pub next_cursor: Option<String>,
}

/// What one re-seal pass did. Counts only; no key names or secrets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResealSummary {
    /// Sealed rows visited.
    pub scanned: usize,
    /// Rows that only the previous key opened and were rewritten.
    pub resealed: usize,
    /// Rows that could not be read or rewritten.
    pub failed: usize,
    /// Cursor for the next page, or `None` when the namespace is exhausted.
    pub next_cursor: Option<String>,
}

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
            return match record::open_sealed_detailing(&value, username, &self.keys) {
                Ok(opened) => {
                    if opened.needs_reseal {
                        // The row predates a key rotation. Rewriting it here
                        // means an address that is still in use moves to the
                        // current key without waiting for an operator.
                        self.reseal_if_unchanged(&key, &value, &opened.record).await;
                    }
                    // The sealed row wins over any plaintext row, but a
                    // leftover must not stay at rest: a migration whose delete
                    // failed leaves both behind, and this is the only place
                    // that can notice.
                    self.clear_legacy_row(username).await;
                    Some(opened.record)
                }
                Err(e) => {
                    log_warn!("LN address record for '{}' is unreadable: {}", username, e);
                    None
                }
            };
        }
        self.migrate_legacy(username).await
    }

    /// Writes `record` as the sealed row for its username, replacing any
    /// earlier row.
    ///
    /// # Errors
    ///
    /// Returns a description when no sealing key is configured, the payload
    /// cannot be sealed, or KV rejects the write. The caller reports the error
    /// without echoing the record.
    pub async fn put_record(&self, record: &LnAddressRecord) -> Result<(), String> {
        let sealed = record::seal(record, &self.keys).map_err(|e| e.to_string())?;
        match self.kv.put(&record::key_for(&record.username), sealed) {
            Ok(builder) => builder.execute().await.map_err(|e| e.to_string())?,
            Err(e) => return Err(e.to_string()),
        }
        // A plaintext row for the same username must not outlive the write.
        self.delete_legacy_row(&record.username).await;
        Ok(())
    }

    /// Deletes both layouts for `username`.
    ///
    /// Returns whether a sealed row was present. The plaintext row is removed
    /// as well, since leaving it behind would make a deleted address readable
    /// again by the migration path.
    pub async fn delete_record(&self, username: &str) -> bool {
        let key = record::key_for(username);
        let existed = matches!(self.kv.get(&key).text().await, Ok(Some(_)));
        if let Err(e) = self.kv.delete(&key).await {
            log_warn!("could not delete the record for '{}': {}", username, e);
        }
        self.delete_legacy_row(username).await;
        existed
    }

    /// Reads one page of sealed registrations, starting at `cursor`.
    ///
    /// Unreadable rows are logged and skipped: one damaged row must not hide
    /// the rest of the namespace from the admin listing. The returned cursor is
    /// `None` once the last page has been read.
    ///
    /// # Errors
    ///
    /// Returns a description when the key listing itself fails. That is a
    /// different case from a damaged row, and it must not be shown as an empty
    /// namespace.
    pub async fn list_records(
        &self,
        cursor: Option<&str>,
    ) -> std::result::Result<RecordPage, &'static str> {
        let page = self.record_keys_page(cursor).await?;
        let mut records = Vec::new();
        for key in page.names {
            let Some(username) = key.strip_prefix(record::KEY_PREFIX) else {
                continue;
            };
            let Some(value) = self.kv.get(&key).text().await.ok().flatten() else {
                continue;
            };
            match record::open_sealed(&value, username, &self.keys) {
                Ok(record) => records.push(record),
                Err(e) => {
                    log_warn!("LN address record for '{}' is unreadable: {}", username, e);
                }
            }
        }
        Ok(RecordPage {
            records,
            next_cursor: page.next_cursor,
        })
    }

    /// Rewrites every row still sealed under the previous key with the current
    /// one, so `LN_ADDRESS_KEY_PREVIOUS` can be retired once the whole
    /// namespace has been walked.
    ///
    /// Rows already under the current key are left untouched. A row that
    /// cannot be read or written is counted and skipped, so one damaged record
    /// does not stall the rest of the rotation. One call works on one page;
    /// pass the returned cursor back to continue where it stopped.
    ///
    /// # Errors
    ///
    /// Returns a description when no sealing key is configured or the key
    /// listing fails. Reporting an empty summary instead would read as "nothing
    /// needed re-sealing" right before an operator retires the previous key.
    pub async fn reseal_all(
        &self,
        cursor: Option<&str>,
    ) -> std::result::Result<ResealSummary, &'static str> {
        if !self.keys.is_configured() {
            log_error!("reseal requested without LN_ADDRESS_KEY; nothing to do");
            return Err("LN address storage is not configured");
        }

        let page = self.record_keys_page(cursor).await?;
        let mut summary = ResealSummary {
            next_cursor: page.next_cursor,
            ..ResealSummary::default()
        };
        for key in page.names {
            let Some(username) = key.strip_prefix(record::KEY_PREFIX) else {
                continue;
            };
            summary.scanned += 1;
            let Some(value) = self.kv.get(&key).text().await.ok().flatten() else {
                summary.failed += 1;
                continue;
            };
            let opened = match record::open_sealed_detailing(&value, username, &self.keys) {
                Ok(opened) => opened,
                Err(e) => {
                    log_warn!("reseal: '{}' is unreadable: {}", username, e);
                    summary.failed += 1;
                    continue;
                }
            };
            if !opened.needs_reseal {
                continue;
            }
            let sealed = match record::seal(&opened.record, &self.keys) {
                Ok(sealed) => sealed,
                Err(e) => {
                    log_warn!("reseal: could not seal '{}': {}", username, e);
                    summary.failed += 1;
                    continue;
                }
            };
            match self.kv.put(&key, sealed) {
                Ok(builder) => match builder.execute().await {
                    Ok(()) => summary.resealed += 1,
                    Err(e) => {
                        log_warn!("reseal: could not store '{}': {}", username, e);
                        summary.failed += 1;
                    }
                },
                Err(e) => {
                    log_warn!("reseal: could not store '{}': {}", username, e);
                    summary.failed += 1;
                }
            }
        }
        Ok(summary)
    }

    /// One page of `lnaddr:` key names, starting at `cursor`.
    ///
    /// # Errors
    ///
    /// Returns [`LIST_FAILED`] when KV cannot be listed.
    async fn record_keys_page(
        &self,
        cursor: Option<&str>,
    ) -> std::result::Result<KeyPage, &'static str> {
        let mut request = self
            .kv
            .list()
            .prefix(record::KEY_PREFIX.to_string())
            .limit(LIST_PAGE_SIZE);
        if let Some(cursor) = cursor {
            request = request.cursor(cursor.to_string());
        }
        let page = match request.execute().await {
            Ok(page) => page,
            Err(e) => {
                log_warn!("could not list LN address keys: {}", e);
                return Err(LIST_FAILED);
            }
        };
        let names: Vec<String> = page.keys.into_iter().map(|key| key.name).collect();
        let next_cursor = if page.list_complete {
            None
        } else if page.cursor.is_none() {
            // A page that says there is more but hands back no cursor cannot
            // be followed; stopping keeps the response honest instead of
            // looping.
            log_warn!("LN address key listing reported more rows but no cursor");
            None
        } else {
            page.cursor
        };
        Ok(KeyPage { names, next_cursor })
    }

    /// Rewrites `record` under the current key if `original` is still the
    /// stored value.
    ///
    /// The comparison closes a race with a concurrent delete: an admin who
    /// removes a registration must not have it recreated by a read that was
    /// already in flight.
    async fn reseal_if_unchanged(&self, key: &str, original: &str, record: &LnAddressRecord) {
        match self.kv.get(key).text().await {
            Ok(Some(current)) if current == original => {}
            _ => return,
        }
        let sealed = match record::seal(record, &self.keys) {
            Ok(sealed) => sealed,
            Err(e) => {
                log_warn!("could not re-seal '{}': {}", record.username, e);
                return;
            }
        };
        let stored = match self.kv.put(key, sealed) {
            Ok(builder) => builder.execute().await,
            Err(e) => Err(e),
        };
        if let Err(e) = stored {
            log_warn!(
                "could not store the re-sealed record for '{}': {}",
                record.username,
                e
            );
        }
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
