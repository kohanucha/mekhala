use std::collections::HashMap;

#[cfg(test)]
use std::cell::RefCell;
use futures::channel::oneshot;
use super::engine::EngineResponse;

/// Abstracts peer I/O and hibernation activation.
/// Production adapter wraps WebSocketRegistry. Test adapter uses in-memory tracking.
pub trait ConnectionTransport {
    /// Platform-specific connection type (e.g. WebSocket).
    type Connection;

    /// Map a platform connection to a numeric connection ID.
    fn identify(&self, conn: &Self::Connection) -> Option<u32>;

    /// Accept a new connection and register it.
    fn accept_and_register(&self, id: u32, conn: &Self::Connection);

    /// Send a message to an external peer (WebSocket).
    /// Returns true if the peer was found and the message was sent.
    fn send_to_peer(&self, id: u32, message: &str) -> bool;

    /// Attempt to wake a hibernated peer by connection ID.
    /// Returns true if the peer was found and re-registered as active.
    fn try_activate(&self, id: u32) -> bool;

    /// Remove a peer from the active set.
    fn remove_peer(&mut self, id: u32);

    /// Count of active (non-hibernated) peers.
    fn active_count(&self) -> usize;

    /// Count of hibernated peers.
    fn hibernated_count(&self) -> usize;

    /// Total count of tracked peers (active + hibernated).
    fn total_count(&self) -> usize {
        self.active_count() + self.hibernated_count()
    }
}

/// Abstracts engine callbacks for connection lifecycle events.
/// Production adapter wraps NostrEngine. Test adapter records calls.
#[async_trait::async_trait(?Send)]
pub trait ConnectionHandler {
    /// Called when a new connection is accepted.
    async fn on_connect(&mut self, connection_id: u32) -> Vec<EngineResponse>;

    /// Load persisted state for a hibernated connection.
    /// Returns true if state was found in storage.
    async fn load(&mut self, connection_id: u32) -> bool;

    /// Called when a connection is terminated.
    async fn on_terminate(&mut self, connection_id: u32);
}

/// Manages oneshot channels for RPC response delivery.
struct InternalConnectionMap {
    channels: HashMap<u32, oneshot::Sender<String>>,
}

impl InternalConnectionMap {
    fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    fn add(&mut self, id: u32, sender: oneshot::Sender<String>) {
        self.channels.insert(id, sender);
    }

    fn send(&mut self, id: u32, message: String) -> bool {
        if let Some(sender) = self.channels.remove(&id) {
            let _ = sender.send(message);
            true
        } else {
            false
        }
    }

    fn remove(&mut self, id: u32) {
        self.channels.remove(&id);
    }
}

/// Deep module that manages connection dispatch and lifecycle.
/// Routes EngineResponse to WebSocket peers or internal RPC channels,
/// and orchestrates hibernation recovery.
pub struct ConnectionManager<T: ConnectionTransport> {
    transport: T,
    internal: InternalConnectionMap,
}

impl<T: ConnectionTransport> ConnectionManager<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            internal: InternalConnectionMap::new(),
        }
    }

    /// Platform-specific connection accessor (test-only probe).
    #[cfg(test)]
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Map a platform connection to a connection ID.
    pub fn identify(&self, conn: &T::Connection) -> Option<u32> {
        self.transport.identify(conn)
    }

    /// Accept a new connection and register it.
    pub fn accept_and_register(&mut self, id: u32, conn: &T::Connection) {
        self.transport.accept_and_register(id, conn);
    }

    /// Attempt to wake a hibernated peer.
    pub fn try_activate(&self, id: u32) -> bool {
        self.transport.try_activate(id)
    }

    /// Total count of tracked peers (active + hibernated).
    pub fn total_count(&self) -> usize {
        self.transport.total_count()
    }

    /// Activate a hibernated peer and load its persisted state.
    pub async fn wake_and_load<H: ConnectionHandler>(&self, id: u32, handler: &mut H) {
        self.transport.try_activate(id);
        let _ = handler.load(id).await;
    }

    /// Dispatch engine responses: send messages and wake hibernated connections.
    pub async fn dispatch<H: ConnectionHandler>(
        &mut self,
        responses: Vec<EngineResponse>,
        handler: &mut H,
    ) {
        for resp in responses {
            match resp {
                EngineResponse::Send { recipient_id, message } => {
                    let json = message.to_json();
                    if !self.transport.send_to_peer(recipient_id, &json) {
                        self.internal.send(recipient_id, json);
                    }
                }
                EngineResponse::WakeUp { connection_id } => {
                    if self.transport.try_activate(connection_id) {
                        let _ = handler.load(connection_id).await;
                    }
                }
            }
        }
    }

    /// Add an internal oneshot channel for RPC responses.
    pub fn add_internal_channel(&mut self, id: u32, sender: oneshot::Sender<String>) {
        self.internal.add(id, sender);
    }

    /// Remove an internal channel.
    pub fn remove_internal_channel(&mut self, id: u32) {
        self.internal.remove(id);
    }

    /// Terminate a connection: clean up engine state, peer, and internal channel.
    /// Ensures hibernated peers are activated before removal.
    pub async fn on_terminate<H: ConnectionHandler>(
        &mut self,
        conn_id: u32,
        handler: &mut H,
    ) {
        handler.on_terminate(conn_id).await;
        self.transport.try_activate(conn_id);
        self.transport.remove_peer(conn_id);
        self.internal.remove(conn_id);
    }
}

#[cfg(test)]
use std::collections::HashSet;

#[cfg(test)]
pub struct MockTransport {
    sent_to_peer: RefCell<HashMap<u32, Vec<String>>>,
    active_peers: RefCell<HashSet<u32>>,
    hibernated_peers: RefCell<HashSet<u32>>,
}

#[cfg(test)]
impl MockTransport {
    pub fn new() -> Self {
        Self {
            sent_to_peer: RefCell::new(HashMap::new()),
            active_peers: RefCell::new(HashSet::new()),
            hibernated_peers: RefCell::new(HashSet::new()),
        }
    }

    pub fn mark_active(&self, id: u32) {
        self.active_peers.borrow_mut().insert(id);
    }

    pub fn mark_hibernated(&self, id: u32) {
        self.hibernated_peers.borrow_mut().insert(id);
    }

    pub fn was_sent_to_peer(&self, id: u32) -> bool {
        self.sent_to_peer.borrow().contains_key(&id)
    }

    pub fn sent_messages(&self, id: u32) -> Vec<String> {
        self.sent_to_peer.borrow().get(&id).cloned().unwrap_or_default()
    }

    pub fn is_active(&self, id: u32) -> bool {
        self.active_peers.borrow().contains(&id)
    }

    pub fn is_hibernated(&self, id: u32) -> bool {
        self.hibernated_peers.borrow().contains(&id)
    }
}

#[cfg(test)]
impl ConnectionTransport for MockTransport {
    type Connection = u32;

    fn identify(&self, conn: &u32) -> Option<u32> {
        if self.active_peers.borrow().contains(conn) || self.hibernated_peers.borrow().contains(conn) {
            Some(*conn)
        } else {
            None
        }
    }

    fn accept_and_register(&self, id: u32, _conn: &u32) {
        self.active_peers.borrow_mut().insert(id);
    }

    fn send_to_peer(&self, id: u32, message: &str) -> bool {
        if self.active_peers.borrow().contains(&id) {
            self.sent_to_peer.borrow_mut()
                .entry(id)
                .or_default()
                .push(message.to_string());
            true
        } else {
            false
        }
    }

    fn try_activate(&self, id: u32) -> bool {
        let was_hibernated = self.hibernated_peers.borrow_mut().remove(&id);
        if was_hibernated {
            self.active_peers.borrow_mut().insert(id);
        }
        was_hibernated
    }

    fn remove_peer(&mut self, id: u32) {
        self.active_peers.borrow_mut().remove(&id);
        self.hibernated_peers.borrow_mut().remove(&id);
    }

    fn active_count(&self) -> usize {
        self.active_peers.borrow().len()
    }

    fn hibernated_count(&self) -> usize {
        self.hibernated_peers.borrow().len()
    }
}

#[cfg(test)]
pub struct MockHandler {
    load_calls: RefCell<Vec<u32>>,
    load_returns: RefCell<HashMap<u32, bool>>,
    terminate_calls: RefCell<Vec<u32>>,
}

#[cfg(test)]
impl MockHandler {
    pub fn new() -> Self {
        Self {
            load_calls: RefCell::new(Vec::new()),
            load_returns: RefCell::new(HashMap::new()),
            terminate_calls: RefCell::new(Vec::new()),
        }
    }

    pub fn was_loaded(&self, id: u32) -> bool {
        self.load_calls.borrow().contains(&id)
    }

    pub fn was_terminated(&self, id: u32) -> bool {
        self.terminate_calls.borrow().contains(&id)
    }
}

#[cfg(test)]
#[async_trait::async_trait(?Send)]
impl ConnectionHandler for MockHandler {
    async fn on_connect(&mut self, _connection_id: u32) -> Vec<EngineResponse> {
        Vec::new()
    }

    async fn load(&mut self, connection_id: u32) -> bool {
        self.load_calls.borrow_mut().push(connection_id);
        *self.load_returns.borrow().get(&connection_id).unwrap_or(&true)
    }

    async fn on_terminate(&mut self, connection_id: u32) {
        self.terminate_calls.borrow_mut().push(connection_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Event;
    use super::super::RelayMessage;
    use futures::executor::block_on;

    fn make_test_event() -> Event {
        Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![],
            content: "test".into(),
            sig: "sig1".into(),
        }
    }

    #[test]
    fn test_dispatch_sends_to_peer() {
        let transport = MockTransport::new();
        transport.mark_active(1);
        let mut manager = ConnectionManager::new(transport);
        let mut handler = MockHandler::new();

        let event = make_test_event();
        let responses = vec![EngineResponse::send(1, RelayMessage::Event("sub1".into(), event))];

        block_on(manager.dispatch(responses, &mut handler));

        assert!(manager.transport().was_sent_to_peer(1));
        assert_eq!(manager.transport().sent_messages(1).len(), 1);
    }

    #[test]
    fn test_dispatch_falls_through_to_internal_channel() {
        let transport = MockTransport::new(); // no active peers
        let mut manager = ConnectionManager::new(transport);
        let (tx, rx) = oneshot::channel();
        manager.add_internal_channel(1, tx);
        let mut handler = MockHandler::new();

        let event = make_test_event();
        let responses = vec![EngineResponse::send(1, RelayMessage::Event("sub1".into(), event))];

        block_on(manager.dispatch(responses, &mut handler));

        assert!(!manager.transport().was_sent_to_peer(1));
        let received = block_on(rx).unwrap();
        assert!(received.contains("EVENT"));
    }

    #[test]
    fn test_dispatch_wakes_hibernated_connection() {
        let transport = MockTransport::new();
        transport.mark_hibernated(42);
        let mut manager = ConnectionManager::new(transport);
        let mut handler = MockHandler::new();

        let responses = vec![EngineResponse::wake_up(42)];
        block_on(manager.dispatch(responses, &mut handler));

        assert!(manager.transport().is_active(42));
        assert!(!manager.transport().is_hibernated(42));
        assert!(handler.was_loaded(42));
    }

    #[test]
    fn test_on_terminate_cleans_up() {
        let transport = MockTransport::new();
        transport.mark_active(1);
        let mut manager = ConnectionManager::new(transport);
        let (tx, _rx) = oneshot::channel();
        manager.add_internal_channel(1, tx);
        let mut handler = MockHandler::new();

        block_on(manager.on_terminate(1, &mut handler));

        assert!(!manager.transport().is_active(1));
        assert!(handler.was_terminated(1));
    }

    #[test]
    fn test_wake_and_load_activates_and_loads() {
        let transport = MockTransport::new();
        transport.mark_hibernated(42);
        let manager = ConnectionManager::new(transport);
        let mut handler = MockHandler::new();

        block_on(manager.wake_and_load(42, &mut handler));

        assert!(manager.transport().is_active(42));
        assert!(!manager.transport().is_hibernated(42));
        assert!(handler.was_loaded(42));
    }

    #[test]
    fn test_on_terminate_activates_hibernated_peer() {
        let transport = MockTransport::new();
        transport.mark_hibernated(1);
        let mut manager = ConnectionManager::new(transport);
        let mut handler = MockHandler::new();

        block_on(manager.on_terminate(1, &mut handler));

        assert!(!manager.transport().is_active(1));
        assert!(!manager.transport().is_hibernated(1));
        assert!(handler.was_terminated(1));
    }

    #[test]
    fn test_try_activate_and_total_count() {
        let transport = MockTransport::new();
        transport.mark_active(1);
        transport.mark_hibernated(2);
        let manager = ConnectionManager::new(transport);

        assert_eq!(manager.total_count(), 2);
        assert!(manager.try_activate(2));
        assert_eq!(manager.total_count(), 2);
        assert!(!manager.try_activate(2)); // already active
    }
}
