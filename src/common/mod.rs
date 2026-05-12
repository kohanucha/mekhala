use futures::channel::oneshot;
use futures_util::FutureExt;

#[async_trait::async_trait(?Send)]
pub trait InternalTransport {
    /// Returns NIP-47 wallet information for a specific pubkey.
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo>;

    /// Injects a message into the engine as a virtual connection, providing the reply channel.
    async fn send_message(&self, id: u32, message: String, sender: oneshot::Sender<String>) -> worker::Result<()>;

    /// Generates a unique ID for a new connection.
    async fn generate_id(&self) -> u32;

    /// Cleans up a virtual connection session.
    async fn close_connection(&self, id: u32) -> worker::Result<()>;
}

pub struct InternalConnection<'a, T: InternalTransport> {
    id: u32,
    transport: &'a T,
}

impl<'a, T: InternalTransport> InternalConnection<'a, T> {
    pub async fn new(transport: &'a T) -> worker::Result<Self> {
        let id = transport.generate_id().await;
        Ok(Self { id, transport })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub async fn send_and_receive(&self, message: String) -> worker::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.transport.send_message(self.id, message, tx).await?;

        let delay = worker::Delay::from(std::time::Duration::from_secs(10)).fuse();
        futures_util::pin_mut!(rx, delay);

        match futures_util::future::select(rx, delay).await {
            futures_util::future::Either::Left((Ok(response), _)) => Ok(response),
            _ => Err(worker::Error::from("Dispatch timeout")),
        }
    }

    pub async fn close(self) -> worker::Result<()> {
        self.transport.close_connection(self.id).await
    }
}
