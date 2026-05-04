use worker::Result;
use async_trait::async_trait;

pub trait SyncTransport {
    fn send(&self, id: u32, message: &str);
}

#[async_trait(?Send)]
pub trait AsyncTransport: Send + Sync {
    async fn send(&self, msg: &str) -> Result<()>;
    async fn receive(&mut self, timeout_ms: u64) -> Result<String>;
}
