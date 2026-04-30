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
    fn get_tagged_websockets(&self, tag: &str) -> Vec<WebSocket>;
    fn is_tags_supported(&self) -> bool;
}

impl HibernationState for State {
    fn set_tags(&self, ws: &WebSocket, tags: Vec<String>) {
        let state_js: &JsValue = unsafe { std::mem::transmute(self) };
        if tags_supported(state_js) {
            let state_ext: &DurableObjectStateExt = state_js.unchecked_ref();
            let tags_array = js_sys::Array::new();
            for tag in tags.into_iter().take(10) {
                tags_array.push(&JsValue::from_str(&tag));
            }
            state_ext.set_websocket_tags_raw(ws.as_ref(), tags_array);
        }
    }

    fn get_tagged_websockets(&self, tag: &str) -> Vec<WebSocket> {
        let state_js: &JsValue = unsafe { std::mem::transmute(self) };
        let mut target_websockets = Vec::new();
        
        if tags_supported(state_js) {
            let state_ext: &DurableObjectStateExt = state_js.unchecked_ref();
            let ws_array = state_ext.get_websockets_raw(Some(tag));
            for ws_js in ws_array.iter() {
                if let Ok(web_sys_ws) = ws_js.dyn_into::<worker::web_sys::WebSocket>() {
                    target_websockets.push(WebSocket::from(web_sys_ws));
                }
            }
        }
        target_websockets
    }

    fn is_tags_supported(&self) -> bool {
        let state_js: &JsValue = unsafe { std::mem::transmute(self) };
        tags_supported(state_js)
    }
}

pub fn tags_supported(state: &JsValue) -> bool {
    js_sys::Reflect::get(state, &JsValue::from_str("setWebSocketTags"))
        .map(|v| !v.is_undefined() && !v.is_null())
        .unwrap_or(false)
}

pub fn set_panic_hook() {
    // When the `console_error_panic_hook` feature is enabled, we can call this
    // function at least once during initialization, and then we will get better
    // error messages if our code ever panics.
    //
    // For more details see
    // https://github.com/rustwasm/console_error_panic_hook#readme
    console_error_panic_hook::set_once();
}

pub fn create_cors_response(response: Response) -> Result<Response> {
    let headers = response.headers().clone();
    headers.set("Access-Control-Allow-Origin", "*")?;
    headers.set("Access-Control-Allow-Methods", "GET, OPTIONS")?;
    headers.set("Access-Control-Allow-Headers", "*")?;
    headers.set("Content-Type", "application/nostr+json")?;
    Ok(response.with_headers(headers))
}

/// Constant-time string comparison to prevent timing attacks
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() { return false; }
    a.bytes().zip(b.bytes()).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Centralized Durable Object stub retrieval
pub fn get_durable_stub(env: &Env, region: Option<String>) -> Result<Stub> {
    let namespace = env.durable_object("NWC_RELAY")?;
    match region {
        Some(r) if !r.is_empty() => namespace.get_by_name_with_location_hint("GLOBAL", &r),
        _ => namespace.id_from_name("GLOBAL")?.get_stub(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq("secret", "secret"));
        assert!(!constant_time_eq("secret", "wrong!!"));
        assert!(!constant_time_eq("secret", "secre"));
        assert!(!constant_time_eq("secret", "secrets"));
        assert!(constant_time_eq("", ""));
    }
}
