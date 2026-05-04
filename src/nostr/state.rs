use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use super::{Filter, Event, Limits};

#[derive(Serialize, Deserialize, Clone)]
pub struct ConnectionState {
    pub id: u32,
    pub subscriptions: HashMap<String, Vec<Filter>>,
    pub info_event: Option<Event>,
    pub limits: Limits,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            id: 0,
            subscriptions: HashMap::new(),
            info_event: None,
            limits: Limits::default(),
        }
    }
}