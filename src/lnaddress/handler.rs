use worker::*;
use crate::cloudflare::create_cors_response;
use crate::lnaddress::bridge::Bridge;
use crate::lnaddress::lnaddress::LNAddress;

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
    
    if !Bridge::user_exists(&ctx, username).await? {
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

    let pr = Bridge::request_invoice(&ctx, username, amount_msat).await?;
    
    let resp = serde_json::json!({
        "pr": pr,
        "routes": []
    });
    create_cors_response(Response::from_json(&resp)?)
}