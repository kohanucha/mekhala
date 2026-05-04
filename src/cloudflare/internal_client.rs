use async_trait::async_trait;
use futures_util::StreamExt;
use worker::*;
use crate::nostr::nip_47::Transport;

pub struct InternalRelayClient {
    stub: Stub,
}

impl InternalRelayClient {
    pub fn new(stub: Stub) -> Self {
        Self { stub }
    }

    pub async fn connect(&self, wallet_pubkey: &str) -> Result<WebSocketTransport> {
        let check_req = Request::new(
            &format!("http://internal/check/{}", wallet_pubkey),
            Method::Get,
        )?;
        let mut check_resp = self.stub.fetch_with_request(check_req).await?;
        if check_resp.text().await? != "OK" {
            return Err(Error::from("Wallet not connected"));
        }

        let mut ws_req = Request::new("http://internal/", Method::Get)?;
        ws_req.headers_mut()?.set("Upgrade", "websocket")?;
        ws_req.headers_mut()?.set("Connection", "Upgrade")?;

        let response = self.stub.fetch_with_request(ws_req).await?;
        let ws = response
            .websocket()
            .ok_or_else(|| Error::from("Failed to upgrade to WebSocket"))?;

        ws.accept()?;

        Ok(WebSocketTransport { ws })
    }
}

pub struct WebSocketTransport {
    ws: WebSocket,
}

#[async_trait(?Send)]
impl Transport for WebSocketTransport {
    async fn send(&self, msg: &str) -> Result<()> {
        self.ws.send_with_str(msg)
    }

    async fn receive(&mut self, _timeout_ms: u64) -> Result<String> {
        let mut stream = self.ws.events()?;
        match stream.next().await {
            Some(Ok(WebsocketEvent::Message(msg))) => msg
                .text()
                .ok_or_else(|| Error::from("Expected text message")),
            Some(Err(e)) => Err(e),
            _ => Err(Error::from("Connection closed")),
        }
    }
}
