use crate::common::UserStore;

pub struct CloudflareKvStore {
    kv: worker::KvStore,
}

impl CloudflareKvStore {
    pub fn new(kv: worker::KvStore) -> Self {
        Self { kv }
    }
}

#[async_trait::async_trait(?Send)]
impl UserStore for CloudflareKvStore {
    async fn get_nwc_uri(&self, username: &str) -> Option<String> {
        self.kv.get(username).text().await.ok().flatten()
    }
}