pub trait GenericTransport {
    fn send(&self, id: u32, message: &str);
    fn persist(&self, id: u32, snapshot: Vec<u8>);
    fn set_tags(&self, id: u32, tags: Vec<String>);
}

pub trait Engine {
    fn on_connect(&mut self, transport: &dyn GenericTransport, id: u32, state: Option<Vec<u8>>);
    fn on_message(&mut self, transport: &dyn GenericTransport, id: u32, message: &str);
    fn on_disconnect(&mut self, transport: &dyn GenericTransport, id: u32);
    fn get_info(&self, path: &str) -> Option<serde_json::Value>;
}
