use worker::*;
use crate::cloudflare::{create_cors_response, get_nwc_uri, connect_internal};
use crate::lnaddress::lnaddress::LNAddress;
use crate::nostr::nip_47::{ConnectionDetails, Session, EncryptionMethod, Transport};

pub async fn handle_lnaddress(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    match handle_lnaddress_inner(req, ctx).await {
        Ok(resp) => Ok(resp),
        Err(e) => lnaddress_error(&e.to_string()),
    }
}

fn lnaddress_error(reason: &str) -> Result<Response> {
    let error_body = serde_json::json!({ "status": "ERROR", "reason": reason });
    create_cors_response(Response::from_json(&error_body)?.with_status(200))
}

async fn handle_lnaddress_inner(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let username = ctx.param("username").ok_or_else(|| Error::from("Missing username"))?;
    
    if get_nwc_uri(&ctx.env, username).await?.is_none() {
        return Err(Error::from("User not found"));
    }

    let ln_address = LNAddress::new(username);

    let url = req.url()?;
    let is_local = url.host_str() == Some("localhost") || url.host_str() == Some("127.0.0.1");
    let host = url.host_str().unwrap_or_default();
    let port = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
    let protocol = if is_local { "http" } else { "https" };
    
    let callback_url = format!("{}://{}{}/lnaddress/{}/callback", protocol, host, port, username);
    let info = ln_address.get_info(&callback_url);

    create_cors_response(Response::from_json(&info)?)
}

pub async fn handle_lnaddress_callback(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    match handle_lnaddress_callback_inner(req, ctx).await {
        Ok(resp) => Ok(resp),
        Err(e) => lnaddress_error(&e.to_string()),
    }
}

async fn handle_lnaddress_callback_inner(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let username = ctx.param("username").ok_or_else(|| Error::from("Missing username"))?;
    
    let url = req.url()?;
    let mut query = url.query_pairs();
    let amount_msat = query
        .find(|(k, _)| k == "amount")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .ok_or_else(|| Error::from("Missing amount"))?;

    let nwc_uri = get_nwc_uri(&ctx.env, username).await?
        .ok_or_else(|| Error::from("User not found"))?;

    let ln_address = LNAddress::new(username);
    let description_hash = ln_address.get_description_hash();

    let pr = request_invoice_internal(&ctx.env, &nwc_uri, amount_msat, description_hash).await?;
    
    let resp = serde_json::json!({
        "pr": pr,
        "routes": []
    });
    create_cors_response(Response::from_json(&resp)?)
}

async fn request_invoice_internal(
    env: &Env,
    nwc_uri: &str,
    amount_msat: u64,
    description_hash: String,
) -> Result<String> {
    let conn = ConnectionDetails::from_uri(nwc_uri)?;
    let mut session = Session::new(conn)?;
    
    let mut transport = connect_internal(env, &session.wallet_pubkey).await?;

    // Discover encryption method via Kind 13194 Info Event
    let sub_id = "discovery";
    let req_msg = serde_json::json!(["REQ", sub_id, {
        "kinds": [13194],
        "authors": [session.wallet_pubkey],
        "limit": 1
    }]).to_string();

    transport.send(&req_msg).await?;
    
    // Wait for the info event or timeout (short 2s timeout for discovery)
    let discovery_res: Result<String> = transport.receive(2000).await;
    if let Ok(msg_text) = discovery_res {
        if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&msg_text) {
            if arr.len() >= 3 && arr[0] == "EVENT" {
                if let Some(tags) = arr[2].get("tags").and_then(|t| t.as_array()) {
                    for tag in tags {
                        if tag.get(0).and_then(|v| v.as_str()) == Some("encryption") {
                            if tag.get(1).and_then(|v| v.as_str()) == Some("nip44_v2") {
                                session.encryption_method = EncryptionMethod::Nip44;
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Close discovery subscription
    let _ = transport.send(&serde_json::json!(["CLOSE", sub_id]).to_string()).await;

    let request_json = serde_json::json!({
        "method": "make_invoice",
        "params": {
            "amount": amount_msat,
            "description_hash": description_hash,
        }
    });

    let resp_json = session.call(&mut transport, &request_json).await?;

    if let Some(result) = resp_json.get("result") {
        if let Some(invoice) = result.get("invoice").and_then(|i| i.as_str()) {
            return Ok(invoice.to_string());
        }
    }

    Err(Error::from("Malformed response: missing invoice result"))
}
