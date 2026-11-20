//! Who may read and change Lightning Address registrations.
//!
//! The admin channel has its own credential and never accepts `RELAY_SECRET`:
//! the relay password is handed to every client that connects, so treating it
//! as an operator credential would let any user register an address.
//!
//! Two credentials are accepted:
//!
//! - `ADMIN_SECRET`, sent as `Authorization: Bearer <secret>`, for an operator
//!   working from a terminal or a page that asks for it.
//! - A Cloudflare Access application token in `Cf-Access-Jwt-Assertion`, for
//!   the browser flow. Access already refuses unauthenticated visitors before
//!   the request reaches the Worker, but the signature, issuer, audience,
//!   token type and lifetime are checked here as well: a policy that is later
//!   widened, or an application that is re-scoped to another path, must not
//!   turn into an open registration endpoint.
//!
//! With neither credential configured the whole channel answers 404, the same
//! answer the relay gives a wrong secret. An unconfigured deployment therefore
//! does not advertise that an admin channel exists.

use crate::auth::access::{AccessClaims, AccessKeys, JwtError, MAX_JWKS_LEN};
use crate::auth::constant_time_eq;
use crate::cloudflare::admin_page;
use crate::cloudflare::config::CloudflareConfig;
use crate::cloudflare::durable_object::get_durable_stub;
use crate::cloudflare::kv::CloudflareKvStore;
use crate::lnaddress::registration;
use crate::log_info;
use crate::log_warn;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use wasm_bindgen::JsValue;
use worker::{Env, Fetch, Method, Request, RequestInit, Response, Result};

/// Header Cloudflare Access adds to an authenticated request.
pub const ACCESS_JWT_HEADER: &str = "Cf-Access-Jwt-Assertion";

/// How long the fetched key document is reused before another fetch.
const KEYS_TTL_SECONDS: u64 = 3_600;

/// Upper bound on the key fetch, so an admin request cannot wait on a slow
/// Access endpoint. Past it the cached keys are used, if any.
const KEYS_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// Longest page cursor accepted from a query string. KV cursors are short, so
/// anything past this did not come from this relay.
const MAX_CURSOR_LEN: usize = 1024;

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminError {
    /// No admin credential is configured, so the channel does not exist.
    NotConfigured,
    /// The caller presented no credential, or a wrong one.
    Unauthorized,
}

/// The Cloudflare Access application the admin channel trusts.
#[derive(Debug, Clone)]
struct AccessApp {
    audience: String,
    team_domain: String,
}

/// Keys fetched from one team's `cdn-cgi/access/certs` endpoint.
struct CachedKeys {
    team_domain: String,
    keys: AccessKeys,
    fetched_at: u64,
}

/// Decides whether an admin request may proceed.
#[derive(Debug, Clone)]
pub struct AdminGuard {
    secret: Option<String>,
    access: Option<AccessApp>,
}

impl AdminGuard {
    /// Reads the admin settings. Access needs both `ADMIN_ACCESS_AUD` and
    /// `ADMIN_ACCESS_TEAM_DOMAIN`; one without the other cannot verify a
    /// token, so it is treated as not configured.
    pub fn from_config(config: &CloudflareConfig) -> Self {
        Self {
            secret: config.admin_secret.clone(),
            access: access_app(
                config.admin_access_aud.clone(),
                config.admin_access_team_domain.clone(),
            ),
        }
    }

    /// Whether any admin credential is configured.
    pub fn is_configured(&self) -> bool {
        self.secret.is_some() || self.access.is_some()
    }

    /// Accepts a bearer secret that matches `ADMIN_SECRET`.
    ///
    /// An empty or missing header never matches, and a configured secret is
    /// always compared in constant time.
    pub fn accepts_secret(&self, header: Option<&str>) -> bool {
        let (Some(expected), Some(provided)) = (self.secret.as_deref(), header) else {
            return false;
        };
        let Some(token) = bearer_token(provided) else {
            return false;
        };
        constant_time_eq(token, expected)
    }

    /// Verifies an Access token against the team's published keys.
    ///
    /// # Errors
    ///
    /// Returns [`JwtError`] when Access is not configured, the token is not
    /// valid for this application right now, or no keys are available.
    pub async fn verify_access(&self, token: &str) -> std::result::Result<AccessClaims, JwtError> {
        let Some(access) = self.access.as_ref() else {
            return Err(JwtError::NoKeys);
        };
        let now = crate::util::now();
        let keys = access.keys(now).await;
        keys.verify(
            token,
            &format!("https://{}", access.team_domain),
            &access.audience,
            now,
        )
    }
}

impl AccessApp {
    /// Current signing keys, from the per-isolate cache when it is fresh.
    ///
    /// A failed fetch falls back to the cached document even past its lifetime:
    /// a token signed minutes ago is still worth accepting when the key
    /// endpoint is briefly unreachable, and refusing every admin request
    /// because of one network blip is worse than trusting keys that were valid
    /// an hour ago.
    async fn keys(&self, now: u64) -> AccessKeys {
        if let Some(cached) = cached_keys().lock().ok().and_then(|guard| {
            guard.as_ref().map(|entry| {
                (
                    entry.team_domain.clone(),
                    entry.keys.clone(),
                    entry.fetched_at,
                )
            })
        }) {
            let (team_domain, keys, fetched_at) = cached;
            if team_domain == self.team_domain && now.saturating_sub(fetched_at) < KEYS_TTL_SECONDS
            {
                return keys;
            }
        }

        match fetch_keys(&self.team_domain).await {
            Some(keys) => {
                if let Ok(mut guard) = cached_keys().lock() {
                    *guard = Some(CachedKeys {
                        team_domain: self.team_domain.clone(),
                        keys: keys.clone(),
                        fetched_at: now,
                    });
                }
                keys
            }
            None => {
                log_warn!("admin: could not fetch Cloudflare Access keys; using the cached set");
                cached_keys()
                    .lock()
                    .ok()
                    .and_then(|guard| {
                        guard
                            .as_ref()
                            .filter(|entry| entry.team_domain == self.team_domain)
                            .map(|entry| entry.keys.clone())
                    })
                    .unwrap_or_default()
            }
        }
    }
}

/// Builds the Access application from its two settings. One without the other
/// cannot verify a token, so it yields `None` and the operator is warned once.
fn access_app(audience: Option<String>, team_domain: Option<String>) -> Option<AccessApp> {
    match (audience, team_domain) {
        (Some(audience), Some(team_domain)) => Some(AccessApp {
            audience,
            team_domain,
        }),
        (None, None) => None,
        _ => {
            warn_partial_access_config();
            None
        }
    }
}

fn cached_keys() -> &'static Mutex<Option<CachedKeys>> {
    static CACHE: OnceLock<Mutex<Option<CachedKeys>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Downloads and parses the team's key document, giving up after
/// [`KEYS_FETCH_TIMEOUT`].
async fn fetch_keys(team_domain: &str) -> Option<AccessKeys> {
    let url = format!("https://{team_domain}/cdn-cgi/access/certs");
    let request = Request::new(&url, Method::Get).ok()?;

    let fetch = Box::pin(async move {
        let mut response = Fetch::Request(request).send().await.ok()?;
        response.text().await.ok()
    });
    let timeout = Box::pin(worker::Delay::from(KEYS_FETCH_TIMEOUT));

    match futures::future::select(fetch, timeout).await {
        futures::future::Either::Left((text, _)) => {
            let text = text?;
            if text.len() > MAX_JWKS_LEN {
                log_warn!("admin: Cloudflare Access key document is too large");
                return None;
            }
            match AccessKeys::from_jwks(&text) {
                Ok(keys) => Some(keys),
                Err(e) => {
                    log_warn!("admin: Cloudflare Access key document is unusable: {}", e);
                    None
                }
            }
        }
        futures::future::Either::Right(((), _)) => {
            log_warn!("admin: Cloudflare Access key fetch timed out");
            None
        }
    }
}

fn warn_partial_access_config() {
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    log_warn!(
        "admin: ADMIN_ACCESS_AUD and ADMIN_ACCESS_TEAM_DOMAIN must both be set; Access sign-in is disabled"
    );
}

/// Extracts the token from an `Authorization: Bearer <token>` header value.
///
/// The scheme is matched case-insensitively, as HTTP requires. A header with
/// no token, or with a different scheme, yields `None`.
///
/// The prefix is compared as bytes: slicing a `&str` at a fixed offset would
/// split a multi-byte character and abort the isolate, and a header is
/// attacker-controlled.
pub fn bearer_token(header: &str) -> Option<&str> {
    const BEARER: &[u8] = b"bearer ";
    let bytes = header.as_bytes();
    if bytes.len() <= BEARER.len() || !bytes[..BEARER.len()].eq_ignore_ascii_case(BEARER) {
        return None;
    }
    // The remainder starts right after an ASCII prefix, so it is still valid
    // UTF-8; the check is free but keeps this function panic-free by shape.
    let token = std::str::from_utf8(&bytes[BEARER.len()..]).ok()?.trim();
    if token.is_empty() {
        return None;
    }
    Some(token)
}

/// A JSON response that no cache may keep.
///
/// # Errors
///
/// Returns an error when the body cannot be serialised or a header is rejected.
pub fn json_response<T: Serialize>(status: u16, body: &T) -> Result<Response> {
    no_store(Response::from_json(body)?.with_status(status))
}

/// Marks a response as private and non-cacheable, and keeps the usual
/// transport hardening headers.
///
/// # Errors
///
/// Returns an error when a header is rejected.
pub fn no_store(response: Response) -> Result<Response> {
    let headers = response.headers().clone();
    headers.set("Cache-Control", "no-store")?;
    headers.set("X-Content-Type-Options", "nosniff")?;
    headers.set(
        "Strict-Transport-Security",
        "max-age=31536000; includeSubDomains",
    )?;
    headers.set("Referrer-Policy", "no-referrer")?;
    Ok(response.with_headers(headers))
}

/// The response for a refused admin request.
///
/// # Errors
///
/// Returns an error when a response cannot be built.
pub fn refusal(error: AdminError) -> Result<Response> {
    match error {
        AdminError::NotConfigured => no_store(Response::error("Not Found", 404)?),
        AdminError::Unauthorized => {
            let response = json_response(401, &serde_json::json!({ "error": "unauthorized" }))?;
            let headers = response.headers().clone();
            headers.set("WWW-Authenticate", "Bearer")?;
            Ok(response.with_headers(headers))
        }
    }
}

/// Logs an accepted Access identity so the operator can see who changed what.
pub fn audit_access(claims: &AccessClaims, action: &str) {
    if let Some(subject) = claims.subject.as_deref() {
        log_info!(
            "admin: '{}' by {}",
            action,
            crate::util::short(subject, 128)
        );
    } else {
        log_info!("admin: '{}' by an unnamed Access identity", action);
    }
}

/// Who presented the credential.
enum Caller {
    Secret,
    Access(AccessClaims),
}

/// Why a registration request is refused before it reaches a wallet.
#[derive(Debug, Deserialize)]
struct RegistrationBody {
    username: String,
    nwc_uri: String,
}

/// Serves the console document.
///
/// The document holds no data and no secret, so it is served without a
/// credential; every byte it shows comes from an authorized API call. The
/// channel still answers 404 while unconfigured, so an unused deployment does
/// not advertise it.
///
/// # Errors
///
/// Returns an error when a response cannot be built.
pub async fn handle_page(env: &Env) -> Result<Response> {
    let config = CloudflareConfig::from_env(env);
    if !AdminGuard::from_config(&config).is_configured() {
        return refusal(AdminError::NotConfigured);
    }

    let mut nonce_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = hex::encode(nonce_bytes);
    let html = admin_page::PAGE.replace(admin_page::NONCE_PLACEHOLDER, &nonce);

    let response = Response::from_html(html)?;
    let headers = response.headers().clone();
    headers.set(
        "Content-Security-Policy",
        &format!(
            "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; \
             connect-src 'self'; img-src 'none'; base-uri 'none'; form-action 'none'; \
             frame-ancestors 'none'"
        ),
    )?;
    headers.set("X-Frame-Options", "DENY")?;
    no_store(response.with_headers(headers))
}

/// Lists the stored registrations without their connection strings.
///
/// # Errors
///
/// Returns an error when a response cannot be built.
pub async fn handle_list(req: &Request, env: &Env) -> Result<Response> {
    if let Err(response) = authorize(req, env).await? {
        return Ok(response);
    }
    let cursor = match page_cursor(req)? {
        Ok(cursor) => cursor,
        Err(response) => return Ok(response),
    };
    let store = match open_store(env) {
        Ok(store) => store,
        Err(reason) => return not_ready(reason),
    };
    let page = match store.list_records(cursor.as_deref()).await {
        Ok(page) => page,
        Err(reason) => return not_ready(reason),
    };
    let views: Vec<serde_json::Value> = page.records.iter().map(registration::summary).collect();
    json_response(
        200,
        &serde_json::json!({ "registrations": views, "next_cursor": page.next_cursor }),
    )
}

/// Registers one Lightning Address after the wallet confirms it grants
/// `pay_invoice`.
///
/// The wallet round trip happens in the Durable Object, which is the only
/// place that can route a NIP-47 request to the wallet and read its cached
/// capabilities. An address that already exists is not replaced: a typed
/// username collision must not silently move someone else's payments to
/// another wallet.
///
/// # Errors
///
/// Returns an error when a response cannot be built.
pub async fn handle_create(mut req: Request, env: &Env) -> Result<Response> {
    let caller = match authorize(&req, env).await? {
        Ok(caller) => caller,
        Err(response) => return Ok(response),
    };

    let body = req.text().await?;
    if body.len() > registration::MAX_SUBMISSION_LEN {
        return json_response(
            413,
            &serde_json::json!({ "error": "request body is too large" }),
        );
    }
    let payload: RegistrationBody = match serde_json::from_str(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return json_response(
                400,
                &serde_json::json!({ "error": "expected a JSON object with username and nwc_uri" }),
            )
        }
    };
    let registration = match registration::parse(&payload.username, &payload.nwc_uri) {
        Ok(registration) => registration,
        Err(e) => return json_response(400, &serde_json::json!({ "error": e.to_string() })),
    };

    let config = CloudflareConfig::from_env(env);
    let stub = match get_durable_stub(env, config.wallet_region.as_deref()) {
        Ok(stub) => stub,
        Err(e) => {
            log_warn!("admin: could not reach the relay object: {}", e);
            return json_response(503, &serde_json::json!({ "error": "relay is unavailable" }));
        }
    };

    let forward = serde_json::json!({
        "username": registration.username,
        "nwc_uri": registration.nwc_uri,
    });
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_body(Some(JsValue::from_str(&forward.to_string())));
    let request = match Request::new_with_init("https://admin.internal/admin/register", &init) {
        Ok(request) => request,
        Err(e) => {
            log_warn!("admin: could not build the registration request: {}", e);
            return json_response(
                500,
                &serde_json::json!({ "error": "could not build the request" }),
            );
        }
    };

    let response = stub.fetch_with_request(request).await?;
    let response = no_store(response)?;
    if response.status_code() == 200 {
        audit(&caller, &format!("registered '{}'", registration.username));
    }
    Ok(response)
}

/// Removes one registration.
///
/// # Errors
///
/// Returns an error when a response cannot be built.
pub async fn handle_delete(req: &Request, env: &Env, username: &str) -> Result<Response> {
    let caller = match authorize(req, env).await? {
        Ok(caller) => caller,
        Err(response) => return Ok(response),
    };
    if !crate::lnaddress::is_valid_username(username) {
        return json_response(404, &serde_json::json!({ "error": "no such registration" }));
    }
    let store = match open_store(env) {
        Ok(store) => store,
        Err(reason) => return not_ready(reason),
    };

    if store.delete_record(username).await {
        audit(&caller, &format!("deleted '{username}'"));
        json_response(200, &serde_json::json!({ "deleted": username }))
    } else {
        json_response(404, &serde_json::json!({ "error": "no such registration" }))
    }
}

/// Rewrites every row still sealed under the previous key.
///
/// # Errors
///
/// Returns an error when a response cannot be built.
pub async fn handle_reseal(req: &Request, env: &Env) -> Result<Response> {
    let caller = match authorize(req, env).await? {
        Ok(caller) => caller,
        Err(response) => return Ok(response),
    };
    let cursor = match page_cursor(req)? {
        Ok(cursor) => cursor,
        Err(response) => return Ok(response),
    };
    let store = match open_store(env) {
        Ok(store) => store,
        Err(reason) => return not_ready(reason),
    };

    let summary = match store.reseal_all(cursor.as_deref()).await {
        Ok(summary) => summary,
        Err(reason) => return not_ready(reason),
    };
    audit(&caller, "re-sealed registrations");
    json_response(
        200,
        &serde_json::json!({
            "scanned": summary.scanned,
            "resealed": summary.resealed,
            "failed": summary.failed,
            "next_cursor": summary.next_cursor,
        }),
    )
}

/// Reads `?cursor=` from an admin request.
///
/// The outer `Result` carries an error only when the request cannot be read;
/// the inner one is `Some(response)` when the cursor must be refused.
///
/// # Errors
///
/// Returns an error when the request URL cannot be read.
fn page_cursor(req: &Request) -> Result<std::result::Result<Option<String>, Response>> {
    let url = req.url()?;
    let cursor = url
        .query_pairs()
        .find(|(name, _)| name == "cursor")
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.is_empty());
    match cursor {
        Some(cursor) if cursor.len() > MAX_CURSOR_LEN => Ok(Err(json_response(
            400,
            &serde_json::json!({ "error": "invalid cursor" }),
        )?)),
        other => Ok(Ok(other)),
    }
}

/// Builds the KV store, or the reason it cannot work.
fn open_store(env: &Env) -> std::result::Result<CloudflareKvStore, &'static str> {
    let config = CloudflareConfig::from_env(env);
    if !config.ln_address_keys.is_configured() {
        return Err("LN address storage is not configured");
    }
    match env.kv("MEKHALA_NWC_KV") {
        Ok(kv) => Ok(CloudflareKvStore::new(kv, config.ln_address_keys)),
        Err(e) => {
            log_warn!("admin: the KV binding is missing: {}", e);
            Err("storage is unavailable")
        }
    }
}

/// The response for an admin request the relay cannot serve right now.
fn not_ready(reason: &str) -> Result<Response> {
    json_response(503, &serde_json::json!({ "error": reason }))
}

/// Authorizes one admin request.
///
/// The outer `Result` carries an error only when a response could not be
/// built; the inner one is the decision.
async fn authorize(req: &Request, env: &Env) -> Result<std::result::Result<Caller, Response>> {
    let config = CloudflareConfig::from_env(env);
    let guard = AdminGuard::from_config(&config);
    if !guard.is_configured() {
        return Ok(Err(refusal(AdminError::NotConfigured)?));
    }

    let headers = req.headers();
    let authorization = headers.get("Authorization").ok().flatten();
    if guard.accepts_secret(authorization.as_deref()) {
        return Ok(Ok(Caller::Secret));
    }
    if let Ok(Some(token)) = headers.get(ACCESS_JWT_HEADER) {
        match guard.verify_access(&token).await {
            Ok(claims) => return Ok(Ok(Caller::Access(claims))),
            Err(e) => {
                log_warn!("admin: Cloudflare Access token refused: {}", e);
            }
        }
    }
    Ok(Err(refusal(AdminError::Unauthorized)?))
}

fn audit(caller: &Caller, action: &str) {
    if let Caller::Access(claims) = caller {
        audit_access(claims, action);
    }
}

#[cfg(test)]
#[path = "admin_test.rs"]
mod admin_test;
