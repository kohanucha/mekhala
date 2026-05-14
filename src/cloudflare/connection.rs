use std::collections::HashMap;
use futures::channel::oneshot;

pub enum Connection<W> {
    Active(W),
    Internal(oneshot::Sender<String>),
}

pub struct ConnectionManager<W> {
    connections: HashMap<u32, Connection<W>>,
}

impl<W> ConnectionManager<W> {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    pub fn add_active(&mut self, id: u32, handle: W) {
        self.connections.insert(id, Connection::Active(handle));
    }

    pub fn add_internal(&mut self, id: u32, sender: oneshot::Sender<String>) {
        self.connections.insert(id, Connection::Internal(sender));
    }

    pub fn get_id<F>(&self, handle: &W, eq: F) -> Option<u32> 
    where 
        F: Fn(&W, &W) -> bool 
    {
        for (id, conn) in &self.connections {
            if let Connection::Active(h) = conn {
                if eq(h, handle) {
                    return Some(*id);
                }
            }
        }
        None
    }

    pub fn send<S>(&mut self, id: u32, message: String, mut sender_fn: S) -> bool 
    where 
        S: FnMut(&W, &str)
    {
        match self.connections.get_mut(&id) {
            Some(Connection::Active(handle)) => {
                sender_fn(handle, &message);
                true
            }
            Some(Connection::Internal(_)) => {
                // We need to take the sender to avoid borrow checker issues if we want to remove it
                if let Some(Connection::Internal(sender)) = self.connections.remove(&id) {
                    let _ = sender.send(message);
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: u32) -> Option<Connection<W>> {
        self.connections.remove(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;

    #[test]
    fn test_connection_manager_active() {
        let mut cm = ConnectionManager::new();
        cm.add_active(1, "ws1");
        
        assert_eq!(cm.get_id(&"ws1", |a, b| a == b), Some(1));
        
        let mut sent = false;
        cm.send(1, "hello".to_string(), |h, m| {
            assert_eq!(*h, "ws1");
            assert_eq!(m, "hello");
            sent = true;
        });
        assert!(sent);
    }

    #[test]
    fn test_connection_manager_internal() {
        let mut cm: ConnectionManager<&str> = ConnectionManager::new();
        let (tx, rx) = oneshot::channel();
        cm.add_internal(2, tx);
        
        assert!(cm.send(2, "ping".to_string(), |_, _| {}));
        
        let resp = rx.now_or_never().unwrap().unwrap();
        assert_eq!(resp, "ping");
        
        // Internal sender should be removed after send
        assert!(!cm.send(2, "ping2".to_string(), |_, _| {}));
    }

    #[test]
    fn test_connection_manager_remove() {
        let mut cm = ConnectionManager::new();
        cm.add_active(1, "ws1");
        assert!(cm.remove(1).is_some());
        assert_eq!(cm.get_id(&"ws1", |a, b| a == b), None);
    }
}
