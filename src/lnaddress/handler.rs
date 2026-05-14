use worker::*;
use crate::cloudflare::create_cors_response;
use crate::lnaddress::LnAddressGateway;

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
    let gateway = LnAddressGateway::new(username);
    
    let info = gateway.get_pay_request_info(&ctx.env, &req.url()?).await?;

    create_cors_response(Response::from_json(&info)?)
}

pub async fn handle_lnaddress_callback(req: Request, env: &Env, username: &str, transport: &impl crate::common::InternalTransport) -> Result<Response> {
    match handle_lnaddress_callback_inner(req, env, username, transport).await {
        Ok(resp) => Ok(resp),
        Err(e) => lnaddress_error(&e.to_string()),
    }
}

async fn handle_lnaddress_callback_inner(req: Request, env: &Env, username: &str, transport: &impl crate::common::InternalTransport) -> Result<Response> {
    let url = req.url()?;
    let mut query = url.query_pairs();
    let amount_msat = query
        .find(|(k, _)| k == "amount")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .ok_or_else(|| Error::from("Missing amount"))?;

    let gateway = LnAddressGateway::new(username);
    let pr = gateway.create_invoice(env, transport, amount_msat).await?;
    
    let resp = serde_json::json!({
        "pr": pr,
        "routes": []
    });
    create_cors_response(Response::from_json(&resp)?)
}
