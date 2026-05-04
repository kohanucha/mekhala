pub trait SyncTransport {
    fn send(&self, id: u32, message: &str);
}
