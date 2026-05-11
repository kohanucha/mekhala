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

pub trait HibernationState {
    fn set_tags(&self, ws: &WebSocket, tags: Vec<String>);
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
}

pub fn tags_supported(state: &JsValue) -> bool {
    js_sys::Reflect::get(state, &JsValue::from_str("setWebSocketTags"))
        .map(|v| !v.is_undefined() && !v.is_null())
        .unwrap_or(false)
}