use worker::*;
use crate::domain::Limits;
use crate::ConnectionState;
use crate::runtime::Platform;
use crate::messages::RelayMessage;

/// Connection manages the lifecycle and handshake of new WebSocket clients.
pub struct Connection;

impl Connection {
    /// Orchestrates the handshake and acceptance of a new WebSocket connection.
    pub fn accept(
        state: &State,
        limits: Limits,
        max_connections: usize,
    ) -> Result<Response> {
        // 1. Enforce global connection limits
        if state.get_websockets().len() >= max_connections {
            return Platform::apply_security_headers(Response::error("Too Many Requests", 429)?);
        }

        // 2. Create the WebSocket pair (client/server)
        let WebSocketPair { client, server } = WebSocketPair::new()?;

        // 3. Initialize persistent ConnectionState with session limits
        let initial_state = ConnectionState {
            limits,
            ..Default::default()
        };
        server.serialize_attachment(&initial_state)?;

        // 4. Accept the connection into the Durable Object hibernation state
        state.accept_web_socket(&server);

        // 5. Return the client-side WebSocket response with security headers
        Response::from_websocket(client)
    }

    /// High-leverage dispatcher for sending multiple structured Nostr messages.
    pub fn send_messages(ws: &WebSocket, messages: Vec<RelayMessage>) {
        for msg in messages {
            let _ = ws.send_with_str(&msg.to_json());
        }
    }

    /// Sends a single message to a WebSocket.
    pub fn send_message(ws: &WebSocket, message: RelayMessage) {
        let _ = ws.send_with_str(&message.to_json());
    }
}
