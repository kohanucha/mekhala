pub mod websocket;
pub mod headers;
pub mod hibernation;
pub mod durable_object;
pub mod kv;
pub mod internal_client;

pub use websocket::Websocket;
pub use headers::{apply_security_headers, create_cors_response};
pub use hibernation::HibernationState;
pub use durable_object::get_durable_stub;
pub use kv::*;
pub use internal_client::*;

#[async_trait::async_trait(?Send)]
pub trait RelayTransport {
    /// Injects a message into the engine and processes resulting responses.
    fn inject_message(&self, id: u32, msg: &str) -> worker::Result<()>;
    
    /// Directly sends a raw string message to a specific connection.
    fn send_raw(&self, id: u32, msg: &str) -> worker::Result<()>;

    /// Loads and returns connection IDs for a pubkey, hydrating from storage if needed.
    async fn load_connections(&self, pubkey: &str) -> worker::Result<Vec<u32>>;
    
    /// Registers a channel for intercepting a specific subscription ID (for async response waiting).
    fn register_dispatch(&self, sub_id: String, sender: futures::channel::oneshot::Sender<String>);

    /// Returns NIP-11 info for a given path if matched.
    async fn get_info(&self, path: &str) -> Option<serde_json::Value>;

    /// Dispatches an NWC event to the target pubkey and waits for a response.
    async fn dispatch_nwc(&self, target_pubkey: &str, event: crate::nostr::Event) -> worker::Result<String>;
}
