use crate::cloudflare::create_cors_response;
use crate::cloudflare::identity::ClientIdentity;
use crate::common::{NwcTransport, UserStore};
use crate::lnaddress::gateway as lnurl;
use crate::lnaddress::policy::{validate_amount, LnAddressPolicy};
use crate::lnaddress::rate_limit::{self, Admission, CounterStore};
use worker::{Error, Request, Response, Result};

pub struct LnAddressHandler<'a, S: UserStore> {
    store: &'a S,
    policy: LnAddressPolicy,
}

impl<'a, S: UserStore> LnAddressHandler<'a, S> {
    pub fn new(store: &'a S, policy: LnAddressPolicy) -> Self {
        Self { store, policy }
    }

    pub async fn handle_pay_request(&self, req: Request, username: &str) -> Result<Response> {
        let nwc_uri = self.store.get_nwc_uri(username).await;
        if nwc_uri.is_none() {
            return lnaddress_error("User not found");
        }

        let info = lnurl::pay_request_info(username, &req.url()?, &self.policy.limits);
        create_cors_response(Response::from_json(&info)?)
    }

    /// Handles one LNURL-pay callback.
    ///
    /// The gates run in this order so each refusal is cheap and provable:
    /// parse the amount, check it against the advertised range, then the
    /// per-source rate limit, and only then touch the store or the wallet.
    pub async fn handle_callback(
        &self,
        req: Request,
        username: &str,
        identity: &ClientIdentity,
        transport: &impl NwcTransport,
        counters: &impl CounterStore,
    ) -> Result<Response> {
        match handle_callback_inner(self, req, username, identity, transport, counters).await {
            Ok(resp) => Ok(resp),
            Err(e) => lnaddress_error(&e.to_string()),
        }
    }

    /// Reads the stored NWC URI for a user straight through the store.
    /// Test-only: the LN address production path reads the store through
    /// `handle_callback_inner` directly.
    #[cfg(test)]
    pub async fn lookup_user(&self, username: &str) -> Option<String> {
        self.store.get_nwc_uri(username).await
    }
}

fn lnaddress_error(reason: &str) -> Result<Response> {
    let error_body = serde_json::json!({ "status": "ERROR", "reason": reason });
    create_cors_response(Response::from_json(&error_body)?.with_status(200))
}

async fn handle_callback_inner<S: UserStore>(
    handler: &LnAddressHandler<'_, S>,
    req: Request,
    username: &str,
    identity: &ClientIdentity,
    transport: &impl NwcTransport,
    counters: &impl CounterStore,
) -> Result<Response> {
    let url = req.url()?;
    let mut query = url.query_pairs();
    let amount_msat = query
        .find(|(k, _)| k == "amount")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .ok_or_else(|| Error::from("Missing amount"))?;

    // Refuse out-of-range amounts before spending a rate-limit slot, a store
    // lookup, or a wallet round trip. The advertised `minSendable`/
    // `maxSendable` are these same bounds.
    validate_amount(amount_msat, &handler.policy.limits).map_err(|e| Error::from(e.to_string()))?;

    // Per-source limiting needs the address tag the Worker stamped on the
    // forwarded request. Without one (local development, or an edge that
    // reports no address) the request is admitted, matching how per-address
    // connection limits are applied.
    if let Some(tag) = identity.ip_tag.as_deref() {
        let key = rate_limit::key_for(tag);
        if let Admission::Limited { retry_after } = rate_limit::admit(
            counters,
            &key,
            crate::util::now(),
            handler.policy.callback_rate_limit_per_minute,
        )
        .await
        {
            return lnaddress_error(&format!("Too many requests, retry in {retry_after}s"));
        }
    }

    let nwc_uri = handler
        .store
        .get_nwc_uri(username)
        .await
        .ok_or_else(|| Error::from("User not found"))?;

    let pr = lnurl::create_invoice(transport, &nwc_uri, username, amount_msat)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    let resp = serde_json::json!({
        "pr": pr,
        "routes": []
    });
    create_cors_response(Response::from_json(&resp)?)
}

#[cfg(test)]
#[path = "handler_test.rs"]
mod handler_test;
