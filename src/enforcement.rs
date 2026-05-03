use worker::*;
use crate::relay::{ClientMessage, RelayMessage};
use crate::router::Router;
use crate::pipeline::EventPipeline;
use crate::connection::Connection;

/// Enforcement encapsulates raw incoming message validation and parsing.
pub struct Enforcement;

impl Enforcement {
    /// Fully orchestrates an incoming WebSocket message (The Gatekeeper).
    pub async fn handle_message(
        ws: &WebSocket,
        message: WebSocketIncomingMessage,
        router: &Router,
        pipeline: &EventPipeline<'_>,
    ) -> Result<()> {
        // 1. Parsing and Raw Validation
        let client_msg = match Self::parse_incoming(&message) {
            Ok(msg) => msg,
            Err(e) => {
                Connection::send_message(ws, RelayMessage::Notice(e));
                return Ok(());
            }
        };

        // 2. Context Retrieval (Lazy Speed Layer)
        let mut conn_state = match router.get_state(ws) {
            Ok(s) => s,
            Err(e) => {
                Connection::send_message(ws, RelayMessage::Notice(format!("error: context failure: {}", e)));
                return Ok(());
            }
        };

        // 3. Pipeline Dispatch (intercepting responses)
        let responses = pipeline.dispatch(ws, &mut conn_state, client_msg).await?;

        // 4. Response Dispatching (The Interceptor/Dispatcher)
        Connection::send_messages(ws, responses);

        Ok(())
    }

    /// Validates and parses an incoming WebSocket message into a structured Nostr message.
    pub fn parse_incoming(message: &WebSocketIncomingMessage) -> Result<ClientMessage, String> {
        if let WebSocketIncomingMessage::String(text) = message {
            // Enforcement: Message size limit (64KB)
            if text.len() > 65536 {
                return Err("error: message too large".into());
            }

            ClientMessage::from_json(text).map_err(|e| format!("error: {}", e))
        } else {
            Err("error: binary messages not supported".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enforcement_size_limit() {
        let large_text = "a".repeat(65537);
        let msg = WebSocketIncomingMessage::String(large_text);
        let res = Enforcement::parse_incoming(&msg);
        assert_eq!(res.unwrap_err(), "error: message too large");

        let ok_text = "a".repeat(65536);
        let msg = WebSocketIncomingMessage::String(ok_text);
        let res = Enforcement::parse_incoming(&msg);
        // Should fail with parse error, but NOT size error
        assert!(res.unwrap_err().contains("parse failed"));
    }

    #[test]
    fn test_enforcement_binary_rejected() {
        let msg = WebSocketIncomingMessage::Binary(vec![0, 1, 2]);
        let res = Enforcement::parse_incoming(&msg);
        assert_eq!(res.unwrap_err(), "error: binary messages not supported");
    }

    #[test]
    fn test_enforcement_parsing() {
        let raw_req = r#"["REQ","sub",{"kinds":[23194]}]"#;
        let msg = WebSocketIncomingMessage::String(raw_req.into());
        let res = Enforcement::parse_incoming(&msg);
        assert!(res.is_ok());

        let malformed = r#"["REQ", "sub", {"kinds": [}]"#;
        let msg = WebSocketIncomingMessage::String(malformed.into());
        let res = Enforcement::parse_incoming(&msg);
        assert!(res.unwrap_err().contains("parse failed"));
    }
}
