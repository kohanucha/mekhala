use futures::lock::Mutex;

/// Seam for persisting ID batch boundaries.
#[async_trait::async_trait(?Send)]
pub(crate) trait IdBatchStore {
    async fn get_last_limit(&self) -> Option<u32>;
    async fn set_next_limit(&self, limit: u32);
}

/// Production adapter wrapping DO storage.
pub(crate) struct DoIdBatchStore {
    storage: worker::Storage,
}

impl DoIdBatchStore {
    pub(crate) fn new(storage: worker::Storage) -> Self {
        Self { storage }
    }
}

#[async_trait::async_trait(?Send)]
impl IdBatchStore for DoIdBatchStore {
    async fn get_last_limit(&self) -> Option<u32> {
        self.storage.get("id_counter").await.ok().flatten()
    }
    async fn set_next_limit(&self, limit: u32) {
        let _ = self.storage.put("id_counter", limit).await;
    }
}

/// Batched ID allocator with persistence for uniqueness across DO hibernation cycles.
///
/// Allocates IDs in batches of 1000 from a persisted counter, minimizing storage
/// writes. Falls back to a timestamp floor on restart to avoid collisions with
/// IDs assigned to still-hibernated connections.
pub(crate) struct IdAllocator<S: IdBatchStore> {
    inner: Mutex<IdCounterState>,
    store: S,
    now: fn() -> u64,
}

struct IdCounterState {
    current_id: u32,
    id_limit: u32,
}

impl<S: IdBatchStore> IdAllocator<S> {
    pub(crate) fn new(store: S) -> Self {
        Self::with_clock(store, crate::util::now)
    }

    pub(crate) fn with_clock(store: S, now: fn() -> u64) -> Self {
        Self {
            inner: Mutex::new(IdCounterState {
                current_id: 0,
                id_limit: 0,
            }),
            store,
            now,
        }
    }

    pub(crate) async fn allocate(&self) -> u32 {
        let mut state = self.inner.lock().await;
        let current = state.current_id;
        let limit = state.id_limit;

        if current >= limit {
            let last_limit = self.store.get_last_limit().await.unwrap_or(0);
            let now = (self.now)() as u32;
            let start = std::cmp::max(last_limit, now);
            let new_limit = start + 1000;
            self.store.set_next_limit(new_limit).await;
            state.current_id = start + 1;
            state.id_limit = new_limit;
            state.current_id
        } else {
            state.current_id = current + 1;
            state.current_id
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct MockStore {
        limit: Cell<Option<u32>>,
        writes: Cell<u32>,
    }

    impl MockStore {
        fn new(initial: Option<u32>) -> Self {
            Self {
                limit: Cell::new(initial),
                writes: Cell::new(0),
            }
        }
    }

    #[async_trait::async_trait(?Send)]
    impl IdBatchStore for MockStore {
        async fn get_last_limit(&self) -> Option<u32> {
            self.limit.get()
        }
        async fn set_next_limit(&self, limit: u32) {
            self.limit.set(Some(limit));
            self.writes.set(self.writes.get() + 1);
        }
    }

    fn test_clock() -> u64 {
        0
    }

    #[test]
    fn test_in_memory_increment() {
        futures::executor::block_on(async {
            let store = MockStore::new(Some(1000));
            let allocator = IdAllocator::with_clock(store, test_clock);

            for i in 1..=100 {
                assert_eq!(allocator.allocate().await, 1000 + i as u32);
            }
        });
    }

    #[test]
    fn test_batch_refill() {
        futures::executor::block_on(async {
            let store = MockStore::new(Some(1000));
            let allocator = IdAllocator::with_clock(store, test_clock);

            for _ in 0..1000 {
                allocator.allocate().await;
            }
            let next = allocator.allocate().await;
            assert_eq!(next, 2001);
        });
    }

    #[test]
    fn test_single_write_per_batch() {
        futures::executor::block_on(async {
            let store = MockStore::new(Some(1000));
            let allocator = IdAllocator::with_clock(store, test_clock);

            for _ in 0..500 {
                allocator.allocate().await;
            }
            assert_eq!(allocator.store.writes.get(), 1);
        });
    }

    fn late_clock() -> u64 {
        8000
    }

    fn early_clock() -> u64 {
        1000
    }

    #[test]
    fn test_timestamp_floor_on_restart() {
        futures::executor::block_on(async {
            let store = MockStore::new(Some(100));
            let allocator = IdAllocator::with_clock(store, late_clock);

            let id = allocator.allocate().await;
            assert_eq!(id, 8001);
        });
    }

    #[test]
    fn test_timestamp_floor_less_than_limit() {
        futures::executor::block_on(async {
            let store = MockStore::new(Some(5000));
            let allocator = IdAllocator::with_clock(store, early_clock);

            let id = allocator.allocate().await;
            assert_eq!(id, 5001);
        });
    }

    #[test]
    fn test_allocations_increase_monotonically() {
        futures::executor::block_on(async {
            let store = MockStore::new(Some(0));
            let allocator = IdAllocator::with_clock(store, test_clock);

            let mut prev = allocator.allocate().await;
            for _ in 0..2000 {
                let next = allocator.allocate().await;
                assert!(next > prev, "IDs must increase monotonically");
                prev = next;
            }
        });
    }
}
