use crate::nostr::Event;

#[async_trait::async_trait(?Send)]
pub trait NwcTransport {
    /// Returns NIP-47 wallet information for a specific pubkey.
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo>;

    /// Sends an NWC request Event and automatically handles the temporary REQ
    /// subscription to wait for the NWC response Event from the wallet.
    async fn execute_nwc_rpc(&self, request: Event) -> worker::Result<Event>;
}
