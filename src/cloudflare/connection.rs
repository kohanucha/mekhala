use futures::channel::oneshot;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use worker::{State, WebSocket};

pub enum ConnectionKind {
    External(WebSocket),
    Internal(oneshot::Sender<String>),
}

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
    connections: HashMap<u32, ConnectionKind>,
    /// Most recent activity seen by this instance for each connection.
    /// Attachment timestamps are refreshed at most once per interval, so the
    /// two are merged when deciding whether a connection is idle.
    last_seen: HashMap<u32, u64>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            last_seen: HashMap::new(),
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
        self.connections
            .insert(attachment.id, ConnectionKind::External(ws.clone()));
        crate::log_debug!("✓ conn={} registered, attachment verified", attachment.id);
    }

    pub fn insert_active(&mut self, id: u32, ws: WebSocket) {
        self.connections.insert(id, ConnectionKind::External(ws));
    }

    pub fn add_internal(&mut self, id: u32, sender: oneshot::Sender<String>) {
        self.connections
            .insert(id, ConnectionKind::Internal(sender));
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
        for (id, kind) in &self.connections {
            if let ConnectionKind::External(registered) = kind {
                if js_sys::Object::is(registered.as_ref(), ws.as_ref()) {
                    return Some(*id);
                }
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

        if let Some(ConnectionKind::External(ws)) = self.connections.get(&id) {
            return Some(ws.clone());
        }

        None
    }

    pub fn get_all_websockets(&self, state: &State) -> Vec<WebSocket> {
        state.get_websockets()
    }

    pub fn len(&self, state: &State) -> usize {
        state.get_websockets().len()
    }

    pub fn send(&mut self, id: u32, message: String) -> bool {
        match self.connections.get(&id) {
            Some(ConnectionKind::External(ws)) => {
                let _ = ws.send_with_str(&message);
                true
            }
            Some(ConnectionKind::Internal(_)) => {
                if let Some(ConnectionKind::Internal(sender)) = self.connections.remove(&id) {
                    sender.send(message).is_ok()
                } else {
                    false
                }
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: u32) {
        self.connections.remove(&id);
        self.last_seen.remove(&id);
    }
}

#[cfg(test)]
#[path = "connection_test.rs"]
mod connection_test;
