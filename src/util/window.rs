//! Fixed-window admission control.
//!
//! A counter gets [`WINDOW_SECONDS`] seconds from its first admitted request to
//! spend `limit` requests. The relay keeps one per connection, in memory, to
//! budget inbound WebSocket frames (`ConnectionRegistry::admit_message`).

/// Length of one admission window, in seconds.
pub const WINDOW_SECONDS: u64 = 60;

/// Requests admitted within the current window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Whole seconds until the current window ends, never `0` so a client is
    /// never told to retry "in 0s".
    pub fn retry_after(&self, now: u64) -> u64 {
        WINDOW_SECONDS
            .saturating_sub(now.saturating_sub(self.window_start))
            .max(1)
    }

    /// Counts one request against `limit` inside this window.
    ///
    /// A `limit` of `0` admits everything without counting, which is how the
    /// budget is switched off. A refused request does not consume a slot, so a
    /// client that backs off still has the remainder of its budget when the
    /// window rolls.
    pub fn admit(&mut self, now: u64, limit: u32) -> Admission {
        if limit == 0 {
            return Admission::Admitted;
        }

        self.roll(now);
        if self.count >= limit {
            return Admission::Limited {
                retry_after: self.retry_after(now),
            };
        }

        self.count += 1;
        Admission::Admitted
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

#[cfg(test)]
#[path = "window_test.rs"]
mod window_test;
