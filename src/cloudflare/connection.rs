use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use worker::{State, WebSocket};

use crate::util::window;

/// What a WebSocket carries with it across hibernation: which connection it
/// is, where the client came from (for admission limits), and when it was
/// last active (for the idle sweep).
///
/// Written with `serialize_attachment`, so it costs no storage operation and
/// is restored when the Durable Object wakes up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// Connection id used for routing.
    pub id: u32,
    /// Opaque tag of the client's address, when the edge reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_tag: Option<String>,
    /// The client's autonomous system number, when the edge reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asn: Option<u32>,
    /// Unix seconds this attachment was last refreshed at.
    pub last_seen: u64,
}

impl Attachment {
    /// Attachment for `id` carrying no identity and no timestamp: the shape
    /// used before identity and activity tracking existed, when the bare
    /// connection id was serialized.
    pub fn legacy(id: u32) -> Self {
        Self {
            id,
            ip_tag: None,
            asn: None,
            last_seen: 0,
        }
    }
}

/// Reads a socket's attachment, falling back to the bare connection id that
/// earlier versions stored.
pub fn attachment_of(ws: &WebSocket) -> Option<Attachment> {
    if let Ok(Some(attachment)) = ws.deserialize_attachment::<Attachment>() {
        return Some(attachment);
    }
    ws.deserialize_attachment::<u32>()
        .ok()
        .flatten()
        .map(Attachment::legacy)
}

/// Connection caps for a new connection. A cap of `0` disables that check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionLimits {
    /// Total accepted connections.
    pub max_connections: usize,
    /// Connections sharing one address tag.
    pub max_per_ip: usize,
    /// Connections sharing one autonomous system.
    pub max_per_asn: usize,
}

/// Verdict on a candidate connection.
#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    /// There is room for the connection.
    Accept,
    /// The client's address tag already holds its share.
    IpAtCapacity,
    /// The client's network already holds its share.
    AsnAtCapacity,
    /// The relay itself is full; a slot has to be freed first.
    Full,
}

/// Decides whether a candidate may connect, given the attachments of the
/// connections already accepted and the total number of sockets held.
pub fn admit(
    existing: &[Attachment],
    total: usize,
    candidate: &Attachment,
    limits: &AdmissionLimits,
) -> Admission {
    if limits.max_per_ip > 0 {
        if let Some(tag) = candidate.ip_tag.as_deref() {
            let used = existing
                .iter()
                .filter(|other| other.ip_tag.as_deref() == Some(tag))
                .count();
            if used >= limits.max_per_ip {
                return Admission::IpAtCapacity;
            }
        }
    }

    if limits.max_per_asn > 0 {
        if let Some(asn) = candidate.asn {
            let used = existing
                .iter()
                .filter(|other| other.asn == Some(asn))
                .count();
            if used >= limits.max_per_asn {
                return Admission::AsnAtCapacity;
            }
        }
    }

    if total >= limits.max_connections {
        return Admission::Full;
    }

    Admission::Accept
}

pub struct ConnectionRegistry {
    /// Sockets this instance has accepted or woken, by connection id.
    connections: HashMap<u32, WebSocket>,
    /// Most recent activity seen by this instance for each connection.
    /// Attachment timestamps are refreshed at most once per interval, so the
    /// two are merged when deciding whether a connection is idle.
    last_seen: HashMap<u32, u64>,
    /// Inbound message window for each connection.
    message_windows: HashMap<u32, window::WindowCounter>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            last_seen: HashMap::new(),
            message_windows: HashMap::new(),
        }
    }

    pub fn accept_and_register(&mut self, state: &State, attachment: &Attachment, ws: &WebSocket) {
        if let Err(e) = ws.serialize_attachment(attachment) {
            crate::log_warn!(
                "serialize_attachment failed for conn={}: {}",
                attachment.id,
                e
            );
        }
        state.accept_web_socket(ws);
        self.last_seen.insert(attachment.id, attachment.last_seen);
        self.connections.insert(attachment.id, ws.clone());
        crate::log_debug!("✓ conn={} registered, attachment verified", attachment.id);
    }

    pub fn insert_active(&mut self, id: u32, ws: WebSocket) {
        self.connections.insert(id, ws);
    }

    /// Records activity on `id`. The in-memory timestamp always moves; the
    /// attachment is refreshed at most once per `refresh` seconds, which is
    /// what a hibernated socket has left to report how long it has been idle.
    pub fn touch(&mut self, ws: &WebSocket, id: u32, now: u64, refresh: u64) {
        let previous = self.last_seen.insert(id, now);
        if previous.is_some_and(|previous| now.saturating_sub(previous) < refresh) {
            return;
        }

        let Some(mut attachment) = attachment_of(ws) else {
            return;
        };
        attachment.last_seen = now;
        if let Err(e) = ws.serialize_attachment(&attachment) {
            crate::log_warn!("serialize_attachment failed for conn={}: {}", id, e);
        }
    }

    /// Most recent activity this instance has seen for `id`.
    pub fn last_seen(&self, id: u32) -> Option<u64> {
        self.last_seen.get(&id).copied()
    }

    pub fn identify(&self, ws: &WebSocket) -> Option<u32> {
        for (id, registered) in &self.connections {
            if js_sys::Object::is(registered.as_ref(), ws.as_ref()) {
                return Some(*id);
            }
        }

        let id = attachment_of(ws)?.id;
        crate::log_debug!(
            "identify: recovered conn={} from hibernation attachment",
            id
        );
        Some(id)
    }

    pub fn find_ws_by_id(&self, state: &State, id: u32) -> Option<WebSocket> {
        for ws in state.get_websockets() {
            if attachment_of(&ws).is_some_and(|attachment| attachment.id == id) {
                return Some(ws);
            }
        }

        self.connections.get(&id).cloned()
    }

    pub fn len(&self, state: &State) -> usize {
        state.get_websockets().len()
    }

    pub fn send(&self, id: u32, message: &str) -> bool {
        match self.connections.get(&id) {
            Some(ws) => {
                let _ = ws.send_with_str(message);
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: u32) {
        self.connections.remove(&id);
        self.last_seen.remove(&id);
        self.message_windows.remove(&id);
    }

    /// Counts one inbound frame against the per-connection budget.
    ///
    /// The window lives in memory. Hibernation drops it, but a socket that
    /// wakes has been silent long enough for a fresh window to be the right
    /// answer, and persisting it would put a storage write in front of every
    /// frame. A reconnect gets a new id and a fresh window, so the admission
    /// caps (`CONN_MAX_PER_IP` / `_PER_ASN`) are what bound how often a
    /// client can start over.
    ///
    /// The caller decides what an exhausted budget means; this only answers.
    pub fn admit_message(&mut self, id: u32, now: u64, limit: u32) -> window::Admission {
        if limit == 0 {
            return window::Admission::Admitted;
        }

        self.message_windows
            .entry(id)
            .or_insert_with(|| window::WindowCounter::new(now))
            .admit(now, limit)
    }
}

/// How many connection ids one storage write reserves.
pub const ID_BLOCK_SIZE: u32 = 1000;

/// Connection ids `first..=limit`, saved as `limit` before any is handed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdBlock {
    pub first: u32,
    pub limit: u32,
}

/// The block after the `stored` limit, never starting below `now`.
///
/// `None` when the ids would overflow `u32`: wrapping would reuse ids that
/// hibernated sockets may still hold.
pub fn reserve_id_block(stored: Option<u32>, now: u64) -> Option<IdBlock> {
    let now = u32::try_from(now).ok()?;
    let start = stored.unwrap_or(0).max(now);
    let limit = start.checked_add(ID_BLOCK_SIZE)?;
    Some(IdBlock {
        first: start.checked_add(1)?,
        limit,
    })
}

#[cfg(test)]
#[path = "connection_test.rs"]
mod connection_test;
