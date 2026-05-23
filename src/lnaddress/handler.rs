use worker::*;
use crate::cloudflare::create_cors_response;
use crate::common::{NwcTransport, UserStore};
use crate::lnaddress::gateway as lnurl;

pub struct LnAddressHandler<'a, S: UserStore> {
    store: &'a S,
}

impl<'a, S: UserStore> LnAddressHandler<'a, S> {
    pub fn new(store: &'a S) -> Self {
        Self { store }
    }

    pub async fn handle_pay_request(&self, req: Request, username: &str) -> Result<Response> {
        let nwc_uri = self.store.get_nwc_uri(username).await;
        if nwc_uri.is_none() {
            return lnaddress_error("User not found");
        }

        let info = lnurl::pay_request_info(username, &req.url()?);
        create_cors_response(Response::from_json(&info)?)
    }

    pub async fn handle_callback(
        &self,
        req: Request,
        username: &str,
        transport: &impl NwcTransport,
    ) -> Result<Response> {
        match handle_callback_inner(self, req, username, transport).await {
            Ok(resp) => Ok(resp),
            Err(e) => lnaddress_error(&e.to_string()),
        }
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
    transport: &impl NwcTransport,
) -> Result<Response> {
    let url = req.url()?;
    let mut query = url.query_pairs();
    let amount_msat = query
        .find(|(k, _)| k == "amount")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .ok_or_else(|| Error::from("Missing amount"))?;

    let nwc_uri = handler.store.get_nwc_uri(username).await
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
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockUserStore {
        uris: HashMap<String, String>,
    }

    #[async_trait::async_trait(?Send)]
    impl UserStore for MockUserStore {
        async fn get_nwc_uri(&self, username: &str) -> Option<String> {
            self.uris.get(username).cloned()
        }
    }

    #[test]
    fn test_lookup_user_found() {
        let mut uris = HashMap::new();
        uris.insert("alice".to_string(), "nostr+walletconnect://pk?secret=s&relay=wss%3A%2F%2Frelay.com".to_string());
        let store = MockUserStore { uris };
        let handler = LnAddressHandler::new(&store);

        futures::executor::block_on(async {
            let result = store.get_nwc_uri("alice").await;
            assert!(result.is_some());
            assert!(result.unwrap().contains("nostr+walletconnect"));
        });
    }

    #[test]
    fn test_lookup_user_not_found() {
        let store = MockUserStore { uris: HashMap::new() };
        let handler = LnAddressHandler::new(&store);

        futures::executor::block_on(async {
            let result = store.get_nwc_uri("nobody").await;
            assert!(result.is_none());
        });
    }

    #[test]
    fn test_lookup_user_empty_uri() {
        let mut uris = HashMap::new();
        uris.insert("bob".to_string(), String::new());
        let store = MockUserStore { uris };
        let handler = LnAddressHandler::new(&store);

        futures::executor::block_on(async {
            let result = store.get_nwc_uri("bob").await;
            assert!(result.is_some());
        });
    }
}