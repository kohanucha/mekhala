use worker::*;
use crate::relay::{ClientMessage};

/// Enforcement encapsulates raw incoming message validation and parsing.
pub struct Enforcement;

impl Enforcement {
    /// Validates and parses an incoming WebSocket message.
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
