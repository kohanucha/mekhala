use worker::*;
use crate::cloudflare::get_durable_stub;

pub async fn info_internal(env: &Env, wallet_pubkey: &str) -> Result<serde_json::Value> {
    let stub = get_durable_stub(env)?;
    let req = Request::new(
        &format!("http://internal/info/{}", wallet_pubkey),
        Method::Get,
    )?;
    let mut resp = stub.fetch_with_request(req).await?;
    resp.json().await
}

pub async fn dispatch_internal(env: &Env, wallet_pubkey: &str, payload: &serde_json::Value) -> Result<String> {
    let stub = get_durable_stub(env)?;
    let mut req = Request::new(
        &format!("http://internal/internal/dispatch/{}", wallet_pubkey),
        Method::Post,
    )?;
    req.headers_mut()?.set("Content-Type", "application/json")?;
    
    let body = serde_json::to_string(payload)?;
    let req = Request::new_with_init(
        &format!("http://internal/internal/dispatch/{}", wallet_pubkey),
        &RequestInit {
            method: Method::Post,
            body: Some(wasm_bindgen::JsValue::from_str(&body)),
            ..Default::default()
        }
    )?;

    let mut resp = stub.fetch_with_request(req).await?;
    if resp.status_code() != 200 {
        let err_text = resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(Error::from(format!("Internal dispatch failed ({}): {}", resp.status_code(), err_text)));
    }
    resp.text().await
}
