use super::transport::SyncTransport;

#[derive(Debug, PartialEq, Eq)]
pub enum EngineAction {
    None,
    Commit,
}

pub struct ConnectionInterests {
    pub pubkeys: Vec<String>,
    pub capabilities: Vec<String>,
}

pub trait Engine {
    fn on_connect(&mut self, transport: &dyn SyncTransport, id: u32, state: Option<Vec<u8>>) -> EngineAction;
    fn on_message(&mut self, transport: &dyn SyncTransport, id: u32, message: &str) -> EngineAction;
    fn on_disconnect(&mut self, transport: &dyn SyncTransport, id: u32) -> EngineAction;
    fn get_info(&self, path: &str) -> Option<serde_json::Value>;
    fn get_interests(&self, id: u32) -> Option<ConnectionInterests>;
    fn get_snapshot(&self, id: u32) -> Option<Vec<u8>>;
    fn initial_state(&self) -> Vec<u8>;
    fn error_message(&self, msg: &str) -> String;
    fn get_connection_id(&self, pubkey: &str) -> Option<u32>;
}
