use worker::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = js_sys::Object)]
    pub type DurableObjectStateExt;

    #[wasm_bindgen(method, js_name = setWebSocketTags)]
    pub fn set_websocket_tags_raw(this: &DurableObjectStateExt, ws: &JsValue, tags: js_sys::Array);

    #[wasm_bindgen(method, js_name = getWebSockets)]
    pub fn get_websockets_raw(this: &DurableObjectStateExt, tag: Option<&str>) -> js_sys::Array;
}

/// Extension trait for worker::State to support the WebSocket Hibernation API
pub trait HibernationState {
    fn set_tags(&self, ws: &WebSocket, tags: Vec<String>);
}

impl HibernationState for State {
    fn set_tags(&self, ws: &WebSocket, tags: Vec<String>) {
        let state_js: &JsValue = unsafe { std::mem::transmute(self) };
        if Platform::tags_supported(state_js) {
            let state_ext: &DurableObjectStateExt = state_js.unchecked_ref();
            let tags_array = js_sys::Array::new();
            for tag in tags.into_iter().take(10) {
                tags_array.push(&JsValue::from_str(&tag));
            }
            state_ext.set_websocket_tags_raw(ws.as_ref(), tags_array);
        }
    }
}

/// Platform encapsulates Cloudflare Worker specific environment interactions.
pub struct Platform;

impl Platform {
    pub fn tags_supported(state: &JsValue) -> bool {
        js_sys::Reflect::get(state, &JsValue::from_str("setWebSocketTags"))
            .map(|v| !v.is_undefined() && !v.is_null())
            .unwrap_or(false)
    }

    pub fn set_panic_hook() {
        console_error_panic_hook::set_once();
    }

    pub fn apply_security_headers(response: Response) -> Result<Response> {
        let headers = response.headers().clone();
        headers.set("Strict-Transport-Security", "max-age=31536000; includeSubDomains")?;
        headers.set("X-Content-Type-Options", "nosniff")?;
        headers.set("Content-Security-Policy", "default-src 'self'")?;
        Ok(response.with_headers(headers))
    }

    pub fn create_cors_response(response: Response) -> Result<Response> {
        let headers = response.headers().clone();
        headers.set("Access-Control-Allow-Origin", "*")?;
        headers.set("Access-Control-Allow-Methods", "GET, OPTIONS")?;
        headers.set("Access-Control-Allow-Headers", "*")?;
        headers.set("Content-Type", "application/nostr+json")?;
        
        Self::apply_security_headers(response.with_headers(headers))
    }

    /// Returns the current timestamp in milliseconds.
    pub fn now_ms() -> u64 {
        #[cfg(target_arch = "wasm32")]
        {
            worker::Date::now().as_millis()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        }
    }

    /// Returns the current timestamp in seconds.
    pub fn now() -> u64 {
        #[cfg(target_arch = "wasm32")]
        {
            (worker::Date::now().as_millis() / 1000) as u64
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        }
    }

    /// Centralized Durable Object stub retrieval
    pub fn get_durable_stub(env: &Env, region: Option<String>) -> Result<Stub> {
        let namespace = env.durable_object("NWC_RELAY")?;
        match region {
            Some(r) if !r.is_empty() => namespace.get_by_name_with_location_hint("GLOBAL", &r),
            _ => namespace.id_from_name("GLOBAL")?.get_stub(),
        }
    }
}
