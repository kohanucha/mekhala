use worker::*;
use crate::nostr::nip_47::{ConnectionDetails, Session, EncryptionMethod};
use crate::cloudflare::{apply_security_headers, RelayTransport};
use serde_json::Value;
use futures::channel::oneshot;
use futures_util::future::{select, Either};
use futures_util::{pin_mut, FutureExt};

pub struct WalletConnector {
    env: Env,
    nwc_uri: String,
}

impl WalletConnector {
    pub fn new(env: &Env, nwc_uri: &str) -> Self {
        Self {
            env: env.clone(),
            nwc_uri: nwc_uri.to_string(),
        }
    }

    pub async fn make_invoice(&self, relay: &impl RelayTransport, amount_msat: u64, description_hash: String) -> Result<String> {
        let conn_details = ConnectionDetails::from_uri(&self.nwc_uri)?;
        let mut session = Session::new(conn_details)?;

        // 1. Check wallet capability via relay get_info
        let info = relay.get_info(&format!("/info/{}", session.wallet_pubkey)).await
            .ok_or_else(|| Error::from("Wallet not connected"))?;
        
        let online = info.get("online").and_then(|v| v.as_bool()).unwrap_or(false);
        if !online {
            return Err(Error::from("Wallet not connected"));
        }

        let ready = info.get("ready").and_then(|v| v.as_bool()).unwrap_or(false);
        if !ready {
            return Err(Error::from("Wallet service info not ready"));
        }

        let encryption_caps = info.get("encryption").and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        // 2. Negotiate encryption: Prefer NIP-44, fallback to NIP-04
        let chosen_scheme = if encryption_caps.contains(&"nip44_v2") {
            session.encryption_method = EncryptionMethod::Nip44;
            "nip44_v2"
        } else {
            session.encryption_method = EncryptionMethod::Nip04;
            "nip04"
        };

        // 3. Create request with mandatory encryption tag
        let request_payload = serde_json::json!({
            "method": "make_invoice",
            "params": {
                "amount": amount_msat,
                "description_hash": description_hash,
            }
        });

        // Build tags according to NIP-47
        let extra_tags = vec![
            vec![Value::String("encryption".into()), Value::String(chosen_scheme.into())],
        ];

        // 4. Dispatch NWC via relay
        let resp_json = session.dispatch(relay, &request_payload, Some(extra_tags)).await?;

        if let Some(result) = resp_json.get("result") {
            if let Some(invoice) = result.get("invoice").and_then(|i| i.as_str()) {
                return Ok(invoice.to_string());
            }
        }
        
        Err(Error::from("Malformed response: missing invoice result"))
    }
}

pub async fn handle_internal_dispatch(relay: &impl RelayTransport, pubkey: &str, mut req: Request) -> Result<Response> {
    let body_text = req.text().await?;
    let event: crate::nostr::Event = serde_json::from_str(&body_text)
        .map_err(|e| Error::from(format!("Invalid NWC Event: {}", e)))?;
    
    let loaded_ids = relay.load_connections(pubkey).await?;

    worker::console_log!("handle_internal_dispatch to {}: loaded IDs {:?}", pubkey, loaded_ids);

    if loaded_ids.is_empty() {
        return apply_security_headers(Response::error("Wallet not connected", 404)?);
    }

    let (tx, rx) = oneshot::channel();
    let sub_id = format!("disp_{}_{}", pubkey.get(..8).unwrap_or(pubkey), worker::js_sys::Math::random().to_string().get(2..10).unwrap_or(""));
    
    relay.register_dispatch(sub_id.clone(), tx);

    // 1. Inject REQ to listen for response
    let primary_id = loaded_ids[0];
    let req_msg = serde_json::json!([
        "REQ",
        sub_id,
        {
            "kinds": [crate::nostr::nip_47::KIND_NWC_RESPONSE],
            "#p": [event.pubkey], 
            "#e": [event.id]
        }
    ]).to_string();
    relay.inject_message(primary_id, &req_msg)?;

    // 2. Inject EVENT (the request) - engine routes to all active subscribers
    let event_msg = serde_json::json!(["EVENT", event]).to_string();
    relay.inject_message(primary_id, &event_msg)?;

    // Wait for response with timeout
    let rx_fuse = rx.fuse();
    let delay = worker::Delay::from(std::time::Duration::from_secs(10)).fuse();
    
    pin_mut!(rx_fuse, delay);

    match select(rx_fuse, delay).await {
        Either::Left((Ok(response), _)) => {
            apply_security_headers(Response::ok(response)?)
        }
        _ => {
            apply_security_headers(Response::error("Dispatch timeout", 504)?)
        }
    }
}
