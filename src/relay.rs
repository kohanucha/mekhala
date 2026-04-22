use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use log::{info, warn, error};
use nostr::{Event, Filter, JsonUtil};

use crate::config::Config;
use crate::whitelist::WhitelistStore;

type WsSink = futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, Message>;

struct ClientState {
    sink: Arc<Mutex<WsSink>>,
    subscriptions: HashMap<String, Vec<Filter>>,
}

type ClientMap = Arc<Mutex<HashMap<String, ClientState>>>;

pub async fn run_server(config: Config, whitelist: Arc<WhitelistStore>) -> Result<(), String> {
    let addr = format!("0.0.0.0:{}", config.relay_port);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind: {}", e))?;

    info!("Relay listening on {}", addr);

    let client_map: ClientMap = Arc::new(Mutex::new(HashMap::new()));

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                let whitelist = whitelist.clone();
                let client_map = client_map.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, peer_addr, whitelist, client_map).await {
                        warn!("Connection error: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    peer_addr: std::net::SocketAddr,
    whitelist: Arc<WhitelistStore>,
    client_map: ClientMap,
) -> Result<(), String> {
    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|e| format!("WebSocket handshake failed: {}", e))?;

    info!("New connection from: {}", peer_addr);

    let (write, mut read) = ws_stream.split();
    let client_id = format!("{}", peer_addr);
    let write = Arc::new(Mutex::new(write));

    {
        let mut clients = client_map.lock().await;
        clients.insert(client_id.clone(), ClientState {
            sink: write.clone(),
            subscriptions: HashMap::new(),
        });
    }

    while let Some(msg_result) = read.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                if let Err(e) = handle_message(&text, &client_id, &whitelist, &client_map).await {
                    warn!("Message handling error for {}: {}", peer_addr, e);
                    // For security reasons, if an unauthorized user connects or sends bad data, we might want to drop them.
                    // The specification says "Disconnect immediately if a non-whitelisted pubkey is requested."
                    break;
                }
            }
            Ok(Message::Close(_)) => {
                info!("Client {} disconnected", peer_addr);
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

    info!("Connection closed: {}", peer_addr);
    Ok(())
}

async fn handle_message(
    text: &str,
    client_id: &str,
    whitelist: &Arc<WhitelistStore>,
    client_map: &ClientMap,
) -> Result<(), String> {
    let msg: Vec<serde_json::Value> = serde_json::from_str(text)
        .map_err(|e| format!("Invalid JSON: {}", e))?;

    if msg.is_empty() {
        return Err("Empty message".to_string());
    }

    let msg_type = msg[0].as_str().ok_or("Message type must be a string")?;

    match msg_type {
        "EVENT" => {
            if msg.len() < 2 {
                return Err("EVENT requires event data".to_string());
            }
            handle_event(msg[1].clone(), client_id, whitelist, client_map).await
        }
        "REQ" => {
            if msg.len() < 3 {
                return Err("REQ requires subscription_id and filters".to_string());
            }
            let sub_id = msg[1].as_str().ok_or("Subscription ID must be a string")?.to_string();
            let filters: Vec<Filter> = msg[2..].iter()
                .filter_map(|f| Filter::from_json(f.to_string()).ok())
                .collect();
            handle_req(filters, &sub_id, client_id, whitelist, client_map).await
        }
        "CLOSE" => {
            if msg.len() < 2 {
                return Err("CLOSE requires subscription_id".to_string());
            }
            let sub_id = msg[1].as_str().ok_or("Subscription ID must be a string")?;
            handle_close(sub_id, client_id, client_map).await
        }
        _ => Err(format!("Unknown message type: {}", msg_type)),
    }
}

async fn handle_event(
    event_val: serde_json::Value,
    client_id: &str,
    whitelist: &Arc<WhitelistStore>,
    client_map: &ClientMap,
) -> Result<(), String> {
    let event = Event::from_json(event_val.to_string())
        .map_err(|e| format!("Invalid event: {}", e))?;

    // Mandatory signature verification
    event.verify().map_err(|e| format!("Invalid signature: {}", e))?;

    let pubkey = event.pubkey.to_string();

    // Whitelist check
    if !whitelist.contains(&pubkey).await? {
        send_ok(client_map, client_id, &event.id.to_string(), false, "restricted: not authorized").await?;
        return Err(format!("Pubkey {} not whitelisted", pubkey));
    }

    send_ok(client_map, client_id, &event.id.to_string(), true, "").await?;

    // Only bridge NWC kinds (and optionally others, but spec says bridging Kinds 23194, 23195)
    let kind = event.kind.as_u16();
    if kind == 23194 || kind == 23195 || kind == 23197 {
        broadcast_to_matching_clients(client_map, client_id, &event).await?;
    }

    Ok(())
}

async fn handle_req(
    filters: Vec<Filter>,
    sub_id: &str,
    client_id: &str,
    whitelist: &Arc<WhitelistStore>,
    client_map: &ClientMap,
) -> Result<(), String> {
    // Security check: ensure authors/p tags only contain whitelisted pubkeys
    for filter in &filters {
        // Check authors
        if let Some(authors) = &filter.authors {
            for author in authors {
                if !whitelist.contains(&author.to_string()).await? {
                    let _ = send_closed(client_map, client_id, sub_id, "restricted: not authorized").await;
                    return Err(format!("Filter author {} not whitelisted", author));
                }
            }
        }

        // Check #p tags
        if let Some(p_tags) = filter.generic_tags.get(&nostr::SingleLetterTag::from_char('p').unwrap()) {
            for p_tag in p_tags {
                let pubkey = p_tag.to_string();
                if !whitelist.contains(&pubkey).await? {
                    let _ = send_closed(client_map, client_id, sub_id, "restricted: not authorized").await;
                    return Err(format!("Filter #p tag {} not whitelisted", pubkey));
                }
            }
        }
    }

    {
        let mut clients = client_map.lock().await;
        if let Some(client) = clients.get_mut(client_id) {
            client.subscriptions.insert(sub_id.to_string(), filters);
        }
    }

    let _ = send_eose(client_map, client_id, sub_id).await;
    Ok(())
}

async fn handle_close(
    sub_id: &str,
    client_id: &str,
    client_map: &ClientMap,
) -> Result<(), String> {
    {
        let mut clients = client_map.lock().await;
        if let Some(client) = clients.get_mut(client_id) {
            client.subscriptions.remove(sub_id);
        }
    }
    let _ = send_closed(client_map, client_id, sub_id, "unsubscribed").await;
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

async fn send_eose(
    client_map: &ClientMap,
    client_id: &str,
    sub_id: &str,
) -> Result<(), String> {
    let response = serde_json::json!(["EOSE", sub_id]);
    send_to_client(client_map, client_id, &response.to_string()).await
}

async fn send_closed(
    client_map: &ClientMap,
    client_id: &str,
    sub_id: &str,
    message: &str,
) -> Result<(), String> {
    let response = serde_json::json!(["CLOSED", sub_id, message]);
    send_to_client(client_map, client_id, &response.to_string()).await
}

async fn broadcast_to_matching_clients(
    client_map: &ClientMap,
    sender_id: &str,
    event: &Event,
) -> Result<(), String> {
    let clients = client_map.lock().await;

    for (client_id, client_state) in clients.iter() {
        if client_id == sender_id {
            continue;
        }

        for (sub_id, filters) in &client_state.subscriptions {
            let mut matches = false;
            for filter in filters {
                if filter.match_event(event, nostr::filter::MatchEventOptions::default()) {
                    matches = true;
                    break;
                }
            }

            if matches {
                let msg_to_send = serde_json::json!(["EVENT", sub_id, event]).to_string();
                let mut sink = client_state.sink.lock().await;
                if let Err(e) = sink.send(Message::Text(msg_to_send)).await {
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
    if let Some(client_state) = clients.get(client_id) {
        let mut sink = client_state.sink.lock().await;
        sink.send(Message::Text(message.to_string())).await
            .map_err(|e| format!("Send error: {}", e))?;
    }
    Ok(())
}