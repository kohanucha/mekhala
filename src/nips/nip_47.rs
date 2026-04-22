#![allow(dead_code)]

use std::sync::Arc;
use tokio::sync::Mutex;
use futures_util::{StreamExt, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use log::warn;
use std::collections::HashMap;

use crate::config::Config;
use crate::whitelist::WhitelistStore;

type WsSink = futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>;
type ClientMap = Arc<Mutex<HashMap<String, Arc<Mutex<WsSink>>>>>;

pub async fn run_nwc_bridge(config: Config, whitelist: Arc<WhitelistStore>) -> Result<(), String> {
    let addr = format!("0.0.0.0:{}", config.relay_port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind: {}", e))?;

    log::info!("NWC Bridge listening on {}", addr);

    let client_map: ClientMap = Arc::new(Mutex::new(HashMap::new()));

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                let whitelist = whitelist.clone();
                let client_map = client_map.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_nwc_connection(stream, peer_addr, whitelist, client_map).await {
                        warn!("Connection error: {}", e);
                    }
                });
            }
            Err(e) => {
                log::error!("Failed to accept connection: {}", e);
            }
        }
    }
}

async fn handle_nwc_connection(
    stream: tokio::net::TcpStream,
    peer_addr: std::net::SocketAddr,
    whitelist: Arc<WhitelistStore>,
    client_map: ClientMap,
) -> Result<(), String> {
    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|e| format!("WebSocket handshake failed: {}", e))?;

    log::info!("New NWC connection from: {}", peer_addr);

    let (write, mut read) = ws_stream.split();
    let client_id = format!("{}", peer_addr);
    let write = Arc::new(Mutex::new(write));

    {
        let mut clients = client_map.lock().await;
        clients.insert(client_id.clone(), write.clone());
    }

    while let Some(msg_result) = read.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                if let Err(e) = handle_nwc_message(&text, &client_id, &whitelist, &client_map).await {
                    warn!("Message handling error for {}: {}", peer_addr, e);
                    break;
                }
            }
            Ok(Message::Close(_)) => {
                log::info!("Client {} disconnected", peer_addr);
                break;
            }
            Err(e) => {
                warn!("Read error from {}: {}", peer_addr, e);
                break;
            }
            _ => {}
        }
    }

    {
        let mut clients = client_map.lock().await;
        clients.remove(&client_id);
    }

    log::info!("NWC connection closed: {}", peer_addr);
    Ok(())
}

async fn handle_nwc_message(
    text: &str,
    client_id: &str,
    whitelist: &Arc<WhitelistStore>,
    client_map: &ClientMap,
) -> Result<(), String> {
    let msg: Vec<String> = serde_json::from_str(text)
        .map_err(|e| format!("Invalid JSON: {}", e))?;

    if msg.is_empty() {
        return Err("Empty message".to_string());
    }

    match msg[0].as_str() {
        "EVENT" => {
            if msg.len() < 2 {
                return Err("EVENT requires event data".to_string());
            }
            handle_event(&msg[1], client_id, whitelist, client_map).await
        }
        "REQ" | "CLOSE" => {
            let _ = (client_id, client_map);
            Ok(())
        }
        _ => Ok(())
    }
}

async fn handle_event(
    event_json: &str,
    client_id: &str,
    whitelist: &Arc<WhitelistStore>,
    client_map: &ClientMap,
) -> Result<(), String> {
    let event: serde_json::Value = serde_json::from_str(event_json)
        .map_err(|e| format!("Invalid event JSON: {}", e))?;

    let pubkey = event["pubkey"]
        .as_str()
        .ok_or("Missing pubkey")?;

    if !whitelist.contains(pubkey).await? {
        send_ok(client_map, client_id, &event["id"].as_str().unwrap_or(""), false, "restricted: not authorized").await?;
        return Err("Not whitelisted".to_string());
    }

    send_ok(client_map, client_id, &event["id"].as_str().unwrap_or(""), true, "").await?;

    let kind = event["kind"].as_i64().unwrap_or(0);
    if kind == 23194 || kind == 23195 || kind == 23197 {
        broadcast_to_matching_clients(client_map, client_id, event_json).await?;
    }

    Ok(())
}

async fn send_ok(
    client_map: &ClientMap,
    client_id: &str,
    event_id: &str,
    ok: bool,
    message: &str,
) -> Result<(), String> {
    let response = serde_json::json!(["OK", event_id, ok, message]);
    send_to_client(client_map, client_id, &response.to_string()).await
}

async fn broadcast_to_matching_clients(
    client_map: &ClientMap,
    sender_id: &str,
    event_json: &str,
) -> Result<(), String> {
    let clients = client_map.lock().await;
    for (client_id, _) in clients.iter() {
        if *client_id != sender_id {
            let msg = Message::Text(event_json.to_string());
            if let Some(write) = clients.get(client_id) {
                let mut write = write.lock().await;
                if let Err(e) = write.send(msg).await {
                    warn!("Failed to send to {}: {}", client_id, e);
                }
            }
        }
    }
    Ok(())
}

async fn send_to_client(
    client_map: &ClientMap,
    client_id: &str,
    message: &str,
) -> Result<(), String> {
    let clients = client_map.lock().await;
    if let Some(write) = clients.get(client_id) {
        let mut write = write.lock().await;
        write.send(Message::Text(message.to_string())).await
            .map_err(|e| format!("Send error: {}", e))?;
    }
    Ok(())
}