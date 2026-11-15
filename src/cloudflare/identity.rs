//! Where a connecting client came from, as far as admission limits go.
//!
//! The Worker sees what the edge knows about a client (`cf.asn()` and the
//! `CF-Connecting-IP` header); the Durable Object that owns the sockets sees
//! neither once the upgrade is forwarded. The Worker therefore stamps the
//! values onto internal headers — overwriting whatever the client sent under
//! the same names — and the Durable Object reads them back.
//!
//! `asn` cannot be chosen by the client: it comes from the edge's own view of
//! the connection, so it is the key that survives address rotation. The
//! address tag is best-effort by comparison: it can only be as trustworthy as
//! `CF-Connecting-IP`, and a Worker cannot prove that header came from the
//! edge rather than from the client. Per-address limits are therefore a
//! comfort measure; per-network limits are the backstop.

use sha2::{Digest, Sha256};
use worker::{Request, Result};

/// Internal header carrying the opaque tag of the client's address.
pub const HEADER_IP_TAG: &str = "x-mekhala-client-ip-tag";
/// Internal header carrying the client's autonomous system number.
pub const HEADER_ASN: &str = "x-mekhala-client-asn";
/// Header Cloudflare sets to the address a request came from.
const HEADER_CONNECTING_IP: &str = "cf-connecting-ip";
/// Bytes of the address digest kept in a tag.
const TAG_BYTES: usize = 8;

/// The identity keys admission limits are enforced against.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    /// Opaque tag of the client's address; `None` when the edge reported no
    /// address, in which case per-address limits cannot be applied.
    pub ip_tag: Option<String>,
    /// The client's autonomous system number.
    pub asn: Option<u32>,
}

/// Reads what the edge reported about the current request.
pub fn from_edge(req: &Request) -> ClientIdentity {
    let address = req
        .headers()
        .get(HEADER_CONNECTING_IP)
        .ok()
        .flatten()
        .filter(|value| !value.is_empty());

    ClientIdentity {
        ip_tag: address.as_deref().map(tag),
        asn: req.cf().and_then(|cf| cf.asn()),
    }
}

/// Reads the identity the Worker stamped on a forwarded request.
pub fn from_headers(req: &Request) -> ClientIdentity {
    ClientIdentity {
        ip_tag: req
            .headers()
            .get(HEADER_IP_TAG)
            .ok()
            .flatten()
            .filter(|value| !value.is_empty()),
        asn: req
            .headers()
            .get(HEADER_ASN)
            .ok()
            .flatten()
            .and_then(|value| value.parse().ok()),
    }
}

/// Stamps the edge's view of the client onto a request forwarded to the
/// Durable Object, replacing any value the client sent under those names.
pub fn stamp(req: &mut Request) -> Result<()> {
    let identity = from_edge(req);
    let headers = req.headers_mut()?;
    headers.delete(HEADER_IP_TAG)?;
    headers.delete(HEADER_ASN)?;
    if let Some(tag) = identity.ip_tag {
        headers.set(HEADER_IP_TAG, &tag)?;
    }
    if let Some(asn) = identity.asn {
        headers.set(HEADER_ASN, &asn.to_string())?;
    }
    Ok(())
}

/// Fixed-length tag for an address. Admission limits only compare tags for
/// equality, so a truncated digest is enough and keeps raw addresses out of
/// socket attachments and logs.
fn tag(address: &str) -> String {
    let digest = Sha256::digest(address.trim().as_bytes());
    hex::encode(&digest[..TAG_BYTES])
}

#[cfg(test)]
#[path = "identity_test.rs"]
mod identity_test;
