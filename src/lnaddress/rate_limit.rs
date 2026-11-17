//! Fixed-window admission control for LN address callbacks.
//!
//! A source gets [`WINDOW_SECONDS`] seconds from its first admitted callback to
//! spend `limit` requests. Counters are only written when a request is
//! admitted, so a source that has exhausted its window costs one read per
//! request and no writes.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Storage key prefix for the per-source callback counters.
pub const RATE_KEY_PREFIX: &str = "rate:";

/// Length of one admission window, in seconds.
pub const WINDOW_SECONDS: u64 = 60;

/// Age after which a counter is removed by the sweep: one full window plus
/// one window of slack, so a counter that just fell out of use is not deleted
/// while a concurrent request might still roll it forward.
pub const STALE_SECONDS: u64 = 2 * WINDOW_SECONDS;

/// Requests admitted within the current window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowCounter {
    /// Unix timestamp (seconds) when the current window started.
    pub window_start: u64,
    /// Requests admitted so far in the current window.
    pub count: u32,
}

impl WindowCounter {
    /// Starts an empty counter whose window begins at `window_start`.
    pub fn new(window_start: u64) -> Self {
        Self {
            window_start,
            count: 0,
        }
    }

    /// Moves the counter to a new window when the current one has elapsed.
    fn roll(&mut self, now: u64) {
        if now.saturating_sub(self.window_start) >= WINDOW_SECONDS {
            self.window_start = now;
            self.count = 0;
        }
    }

    /// Whether the counter has been out of use long enough to be removed.
    pub fn is_stale(&self, now: u64) -> bool {
        now.saturating_sub(self.window_start) >= STALE_SECONDS
    }

    /// Whole seconds until the current window ends, never `0` so a client is
    /// never told to retry "in 0s".
    pub fn retry_after(&self, now: u64) -> u64 {
        WINDOW_SECONDS
            .saturating_sub(now.saturating_sub(self.window_start))
            .max(1)
    }
}

/// One admission decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The request was counted and may proceed.
    Admitted,
    /// The window is full; `retry_after` is in whole seconds.
    Limited {
        /// Seconds until the current window ends.
        retry_after: u64,
    },
}

/// Persistence seam for the per-source counters.
#[async_trait(?Send)]
pub trait CounterStore {
    /// Reads the counter stored under `key`, or `None` when it is absent or
    /// unreadable.
    async fn load(&self, key: &str) -> Option<WindowCounter>;

    /// Persists `counter` under `key`.
    async fn save(&self, key: &str, counter: &WindowCounter);
}

/// Storage key for one source tag.
pub fn key_for(ip_tag: &str) -> String {
    format!("{RATE_KEY_PREFIX}{ip_tag}")
}

/// Decodes a stored counter; unreadable values are reported as `None` so the
/// sweep can drop them.
pub fn decode(value: &serde_json::Value) -> Option<WindowCounter> {
    serde_json::from_value(value.clone()).ok()
}

/// Counts one request against `limit` for `key`.
///
/// A `limit` of `0` admits everything without touching storage, which is how
/// the limiter is disabled.
pub async fn admit<S: CounterStore>(store: &S, key: &str, now: u64, limit: u32) -> Admission {
    if limit == 0 {
        return Admission::Admitted;
    }

    let mut counter = store
        .load(key)
        .await
        .unwrap_or_else(|| WindowCounter::new(now));
    counter.roll(now);

    if counter.count >= limit {
        return Admission::Limited {
            retry_after: counter.retry_after(now),
        };
    }

    counter.count += 1;
    store.save(key, &counter).await;
    Admission::Admitted
}

#[cfg(test)]
#[path = "rate_limit_test.rs"]
mod rate_limit_test;
