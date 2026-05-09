use worker::*;

pub fn get_durable_stub(env: &Env) -> Result<Stub> {
    let namespace = env.durable_object("NWC_RELAY")?;
    let region = env.var("WALLET_REGION").map(|v| v.to_string()).ok();
    match region {
        Some(r) if !r.is_empty() => namespace.get_by_name_with_location_hint("GLOBAL", &r),
        _ => namespace.id_from_name("GLOBAL")?.get_stub(),
    }
}

#[async_trait::async_trait(?Send)]
impl crate::cloudflare::RelayTransport for Stub {
    fn inject_message(&self, _id: u32, _msg: &str) -> Result<()> {
        Err(Error::from("inject_message not supported on stub"))
    }
    
    fn send_raw(&self, _id: u32, _msg: &str) -> Result<()> {
        Err(Error::from("send_raw not supported on stub"))
    }

    async fn load_connections(&self, _pubkey: &str) -> Result<Vec<u32>> {
        Err(Error::from("load_connections not supported on stub"))
    }
    
    fn register_dispatch(&self, _sub_id: String, _sender: futures::channel::oneshot::Sender<String>) {
        // No-op
    }

    async fn get_info(&self, path: &str) -> Option<serde_json::Value> {
        let req = Request::new(
            &format!("http://internal{}", path),
            Method::Get,
        ).ok()?;
        
        let mut resp = self.fetch_with_request(req).await.ok()?;
        if resp.status_code() == 200 {
            resp.json().await.ok()
        } else {
            None
        }
    }

    async fn dispatch_nwc(&self, target_pubkey: &str, event: crate::nostr::Event) -> Result<String> {
        let body = serde_json::to_string(&event)?;
        let req = Request::new_with_init(
            &format!("http://internal/internal/dispatch/{}", target_pubkey),
            &RequestInit {
                method: Method::Post,
                body: Some(wasm_bindgen::JsValue::from_str(&body)),
                ..Default::default()
            }
        )?;

        let mut resp = self.fetch_with_request(req).await?;
        if resp.status_code() != 200 {
            let err_text = resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(Error::from(format!("Internal dispatch failed ({}): {}", resp.status_code(), err_text)));
        }
        resp.text().await
    }
}
