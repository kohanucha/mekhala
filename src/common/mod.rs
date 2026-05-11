#[async_trait::async_trait(?Send)]
pub trait InternalTransport {
    /// Loads and returns a connection ID for a pubkey, hydrating from storage if needed.
    async fn load_connection(&self, pubkey: &str) -> worker::Result<Option<u32>>;
    
    /// Returns NIP-47 wallet information for a specific pubkey.
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo>;

    /// Formally creates a virtual connection session.
    async fn create_connection(&self) -> worker::Result<u32>;

    /// Sends a message from a specific virtual connection.
    async fn send_message(&self, id: u32, message: String) -> worker::Result<()>;

    /// Waits for a response message signaled to a specific virtual connection.
    async fn receive_message(&self, id: u32) -> worker::Result<String>;

    /// Cleans up a virtual connection session.
    async fn close_connection(&self, id: u32) -> worker::Result<()>;
}
