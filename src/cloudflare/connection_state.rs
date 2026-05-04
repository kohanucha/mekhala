use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::nostr::Filter;
use crate::nostr::Event;
use crate::limits::Limits;

#[derive(Serialize, Deserialize, Clone)]
pub struct ConnectionState {
    pub subscriptions: HashMap<String, Vec<Filter>>,
    pub info_event: Option<Event>,
    pub limits: Limits,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            subscriptions: HashMap::new(),
            info_event: None,
            limits: Limits::default(),
        }
    }
}