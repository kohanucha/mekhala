#[derive(Default, Debug, PartialEq, Eq)]
pub struct EngineResponse {
    pub connection_ids: Vec<u32>,
    pub messages: String,
}

impl EngineResponse {
    pub fn new(connection_id: u32, messages: String) -> Self {
        Self {
            connection_ids: vec![connection_id],
            messages,
        }
    }

    pub fn multi(connection_ids: Vec<u32>, messages: String) -> Self {
        Self {
            connection_ids,
            messages,
        }
    }
}

pub trait Engine {
    fn on_connect(&mut self, id: u32) -> Vec<EngineResponse>;
    fn on_reconnect(&mut self, id: u32, state: Vec<u8>);
    fn on_message(&mut self, id: u32, message: &str) -> Vec<EngineResponse>;
    fn on_disconnect(&mut self, id: u32) -> Vec<EngineResponse>;
    fn get_info(&self, path: &str) -> Option<serde_json::Value>;
    fn initial_state(&self) -> Vec<u8>;
    fn error_message(&self, msg: &str) -> String;
    fn get_connection_ids(&self, pubkey: &str) -> Vec<u32>;
    fn get_target_pubkeys(&self, message: &str) -> Option<Vec<String>>;
    fn export_state(&self, id: u32) -> Option<serde_json::Value>;
    fn import_state(&mut self, id: u32, data: serde_json::Value);
}
