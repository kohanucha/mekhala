//! LN address callback policy: the sendable amount range and the per-source
//! callback rate limit.
//!
//! The range advertised by the `payRequest` document and the range enforced on
//! the callback come from the same [`SendableLimits`], so a client cannot be
//! told one thing and refused for another.

use crate::log_warn;
use std::sync::atomic::{AtomicBool, Ordering};

/// Minimum sendable amount used when the configured value is unusable.
pub const DEFAULT_MIN_SENDABLE_MSAT: u64 = 1_000;

/// Maximum sendable amount used when the configured value is unusable.
pub const DEFAULT_MAX_SENDABLE_MSAT: u64 = 100_000_000;

/// Inclusive range of invoice amounts, in millisatoshis.
///
/// LUD-06 defines `minSendable`/`maxSendable` as a service promise, so the
/// range is always enforced. A zero bound would have to be advertised as `0`
/// (which clients do not treat as "unbounded") and an inverted range cannot be
/// satisfied at all, so both fall back to the defaults instead of disabling
/// the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendableLimits {
    min_msat: u64,
    max_msat: u64,
}

impl SendableLimits {
    /// Builds the range, replacing a zero bound or an inverted range with the
    /// defaults and warning once per isolate.
    pub fn new(min_msat: u64, max_msat: u64) -> Self {
        if min_msat == 0 || max_msat == 0 || min_msat > max_msat {
            warn_invalid_range_once(min_msat, max_msat);
            return Self::default();
        }
        Self { min_msat, max_msat }
    }

    /// Lowest accepted amount, in millisatoshis (inclusive).
    pub fn min_msat(&self) -> u64 {
        self.min_msat
    }

    /// Highest accepted amount, in millisatoshis (inclusive).
    pub fn max_msat(&self) -> u64 {
        self.max_msat
    }
}

impl Default for SendableLimits {
    fn default() -> Self {
        Self {
            min_msat: DEFAULT_MIN_SENDABLE_MSAT,
            max_msat: DEFAULT_MAX_SENDABLE_MSAT,
        }
    }
}

/// Why a callback amount was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AmountError {
    /// The amount is below the advertised minimum; carries that minimum.
    #[error("Amount below minimum ({0} msat)")]
    BelowMinimum(u64),
    /// The amount is above the advertised maximum; carries that maximum.
    #[error("Amount above maximum ({0} msat)")]
    AboveMaximum(u64),
}

/// Checks `amount_msat` against the advertised range.
///
/// # Errors
///
/// Returns [`AmountError::BelowMinimum`] or [`AmountError::AboveMaximum`] when
/// the amount falls outside the inclusive range.
pub fn validate_amount(amount_msat: u64, limits: &SendableLimits) -> Result<(), AmountError> {
    if amount_msat < limits.min_msat() {
        return Err(AmountError::BelowMinimum(limits.min_msat()));
    }
    if amount_msat > limits.max_msat() {
        return Err(AmountError::AboveMaximum(limits.max_msat()));
    }
    Ok(())
}

fn warn_invalid_range_once(min_msat: u64, max_msat: u64) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if WARNED.swap(true, Ordering::Relaxed) {
        return;
    }
    log_warn!(
        "LN address amount range {min_msat}..{max_msat} is unusable; using {DEFAULT_MIN_SENDABLE_MSAT}..{DEFAULT_MAX_SENDABLE_MSAT}"
    );
}

/// Everything the LN address routes need from configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LnAddressPolicy {
    /// Advertised and enforced invoice amount range.
    pub limits: SendableLimits,
    /// Admitted callbacks per source per minute; `0` disables the limit.
    pub callback_rate_limit_per_minute: u32,
}

impl LnAddressPolicy {
    /// Builds the policy, sanitising the amount range (see
    /// [`SendableLimits::new`]).
    pub fn new(
        min_amount_msat: u64,
        max_amount_msat: u64,
        callback_rate_limit_per_minute: u32,
    ) -> Self {
        Self {
            limits: SendableLimits::new(min_amount_msat, max_amount_msat),
            callback_rate_limit_per_minute,
        }
    }
}

#[cfg(test)]
#[path = "policy_test.rs"]
mod policy_test;
