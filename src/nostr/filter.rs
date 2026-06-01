use serde::{Deserialize, Serialize};
use crate::nostr::Event;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Filter {
    pub ids: Option<Vec<String>>,
    pub authors: Option<Vec<String>>,
    pub kinds: Option<Vec<u64>>,
    #[serde(rename = "#p")]
    pub p_tags: Option<Vec<String>>,
    #[serde(rename = "#e")]
    pub e_tags: Option<Vec<String>>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: Option<u64>,
}

impl Filter {
    pub fn matches(&self, event: &Event) -> bool {
        if let Some(ids) = &self.ids {
            if !ids.contains(&event.id) {
                return false;
            }
        }
        if let Some(authors) = &self.authors {
            if !authors.contains(&event.pubkey) {
                return false;
            }
        }
        if let Some(kinds) = &self.kinds {
            if !kinds.contains(&event.kind) {
                return false;
            }
        }
        if let Some(p_tags) = &self.p_tags {
            let has_match = event.tags.iter().any(|t| {
                t.pubkey().is_some_and(|pk| p_tags.iter().any(|s| s == pk))
            });
            if !has_match {
                return false;
            }
        }
        if let Some(e_tags) = &self.e_tags {
            let has_match = event.tags.iter().any(|t| {
                t.event_id().is_some_and(|eid| e_tags.iter().any(|s| s == eid))
            });
            if !has_match {
                return false;
            }
        }
        if let Some(since) = self.since {
            if event.created_at < since {
                return false;
            }
        }
        if let Some(until) = self.until {
            if event.created_at > until {
                return false;
            }
        }
        true
    }

    pub fn is_valid(&self) -> bool {
        let has_specific_narrowing = self.p_tags.is_some() || self.e_tags.is_some() || self.ids.is_some();

        if has_specific_narrowing {
            if let Some(kinds) = &self.kinds {
                if kinds.iter().any(|k| !matches!(k, 5 | 13194 | 23194..=23197)) {
                    return false;
                }
            }
        } else {
            let kinds = match &self.kinds {
                Some(k) if !k.is_empty() => k,
                _ => return false,
            };

            if kinds.iter().any(|k| !matches!(k, 5 | 13194 | 23194..=23197)) {
                return false;
            }

            if self.ids.is_none() && self.authors.is_none() && self.p_tags.is_none() && self.e_tags.is_none() {
                return false;
            }
        }

        true
    }

    pub fn pubkeys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if let Some(authors) = &self.authors {
            keys.extend(authors.clone());
        }
        if let Some(p_tags) = &self.p_tags {
            keys.extend(p_tags.clone());
        }
        keys
    }
}

#[cfg(test)]
#[path = "filter_test.rs"]
mod filter_test;