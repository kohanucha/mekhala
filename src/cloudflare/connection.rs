use worker::*;
use crate::nostr::state::ConnectionState;
use crate::cloudflare::apply_security_headers;

pub fn accept_connection(state: &State, max_connections: usize) -> Result<Response> {
    if state.get_websockets().len() >= max_connections {
        return apply_security_headers(Response::error("Too Many Requests", 429)?);
    }

    let WebSocketPair { client, server } = WebSocketPair::new()?;

    let initial_state = ConnectionState::default();
    server.serialize_attachment(&initial_state)?;

    state.accept_web_socket(&server);

    Response::from_websocket(client)
}