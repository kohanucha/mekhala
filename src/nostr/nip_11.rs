use crate::cloudflare::create_cors_response;
use worker::*;

pub fn handle_get_info() -> Result<Response> {
    let info = serde_json::json!({
        "supported_nips": [1, 11, 47]
    });
    create_cors_response(Response::from_json(&info)?)
}
