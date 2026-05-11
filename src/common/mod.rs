#[async_trait::async_trait(?Send)]
pub trait Transport {
    /// Loads and returns a connection ID for a pubkey, hydrating from storage if needed.
    async fn load_connection(&self, pubkey: &str) -> worker::Result<Option<u32>>;
    
    /// Returns NIP-47 wallet information for a specific pubkey.
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo>;

    /// Dispatches generic messages to a specific connection and waits for a response matching client_id.
    async fn dispatch(&self, connection_id: u32, client_id: &str, messages: Vec<String>) -> worker::Result<String>;
}
