use serde::{Deserialize, Serialize};
use crate::nostr::Event;
use crate::nostr::Limits;

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
                t.len() >= 2
                    && t[0].as_str() == Some("p")
                    && t[1].as_str().map_or(false, |val| p_tags.iter().any(|s| s == val))
            });
            if !has_match {
                return false;
            }
        }
        if let Some(e_tags) = &self.e_tags {
            let has_match = event.tags.iter().any(|t| {
                t.len() >= 2
                    && t[0].as_str() == Some("e")
                    && t[1].as_str().map_or(false, |val| e_tags.iter().any(|s| s == val))
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

    pub fn is_valid(&self, limits: &Limits) -> bool {
        
        // Enforce narrowing (must have at least one of: ids, authors, #p, #e)
        if self.ids.is_none() && self.authors.is_none() && self.p_tags.is_none() && self.e_tags.is_none() {
            return false;
        }

        if let Some(ids) = &self.ids {
            if ids.len() > limits.max_filter_items {
                return false;
            }
        }
        if let Some(authors) = &self.authors {
            if authors.len() > limits.max_filter_items {
                return false;
            }
        }
        if let Some(kinds) = &self.kinds {
            if kinds.len() > limits.max_filter_items {
                return false;
            }
        }
        if let Some(p_tags) = &self.p_tags {
            if p_tags.len() > limits.max_filter_items {
                return false;
            }
        }
        if let Some(e_tags) = &self.e_tags {
            if e_tags.len() > limits.max_filter_items {
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
mod tests {
    use super::*;
    use crate::nostr::Event;

    fn make_event(id: &str, pubkey: &str, kind: u64, tags: Vec<Vec<serde_json::Value>>, created_at: u64) -> Event {
        Event {
            id: id.into(),
            pubkey: pubkey.into(),
            kind,
            tags,
            content: "test".into(),
            sig: "sig".into(),
            created_at,
        }
    }

    #[test]
    fn test_filter_matches_ids() {
        let filter = Filter {
            ids: Some(vec!["id1".into()]),
            ..Default::default()
        };
        let event = make_event("id1", "author1", 1, vec![], 1000);
        assert!(filter.matches(&event));
        
        let event = make_event("id2", "author1", 1, vec![], 1000);
        assert!(!filter.matches(&event));
    }

    #[test]
    fn test_filter_matches_authors() {
        let filter = Filter {
            authors: Some(vec!["author1".into()]),
            ..Default::default()
        };
        let event = make_event("id1", "author1", 1, vec![], 1000);
        assert!(filter.matches(&event));
        
        let event = make_event("id1", "author2", 1, vec![], 1000);
        assert!(!filter.matches(&event));
    }

    #[test]
    fn test_filter_matches_kinds() {
        let filter = Filter {
            kinds: Some(vec![1, 2]),
            ..Default::default()
        };
        let event = make_event("id1", "author1", 1, vec![], 1000);
        assert!(filter.matches(&event));
        
        let event = make_event("id1", "author1", 3, vec![], 1000);
        assert!(!filter.matches(&event));
    }

    #[test]
    fn test_filter_matches_since_until() {
        let filter = Filter {
            since: Some(1000),
            until: Some(2000),
            ..Default::default()
        };
        let event = make_event("id1", "author1", 1, vec![], 1500);
        assert!(filter.matches(&event));
        
        let event = make_event("id1", "author1", 1, vec![], 500);
        assert!(!filter.matches(&event));
        
        let event = make_event("id1", "author1", 1, vec![], 2500);
        assert!(!filter.matches(&event));
    }

    #[test]
    fn test_filter_matches_p_tags() {
        let filter = Filter {
            p_tags: Some(vec!["pubkey1".into()]),
            ..Default::default()
        };
        let event = make_event("id1", "author1", 1, vec![
            vec![serde_json::Value::String("p".into()), serde_json::Value::String("pubkey1".into())]
        ], 1000);
        assert!(filter.matches(&event));
        
        let event = make_event("id1", "author1", 1, vec![
            vec![serde_json::Value::String("p".into()), serde_json::Value::String("pubkey2".into())]
        ], 1000);
        assert!(!filter.matches(&event));
    }

    #[test]
    fn test_filter_matches_e_tags() {
        let filter = Filter {
            e_tags: Some(vec!["event1".into()]),
            ..Default::default()
        };
        let event = make_event("id1", "author1", 1, vec![
            vec![serde_json::Value::String("e".into()), serde_json::Value::String("event1".into())]
        ], 1000);
        assert!(filter.matches(&event));
        
        let event = make_event("id1", "author1", 1, vec![
            vec![serde_json::Value::String("e".into()), serde_json::Value::String("event2".into())]
        ], 1000);
        assert!(!filter.matches(&event));
    }

    #[test]
    fn test_filter_matches_all_criteria() {
        let filter = Filter {
            ids: Some(vec!["id1".into()]),
            authors: Some(vec!["author1".into()]),
            kinds: Some(vec![1]),
            since: Some(500),
            until: Some(2000),
            ..Default::default()
        };
        let event = make_event("id1", "author1", 1, vec![], 1000);
        assert!(filter.matches(&event));
        
        let event = make_event("id2", "author1", 1, vec![], 1000);
        assert!(!filter.matches(&event));
        
        let event = make_event("id1", "author2", 1, vec![], 1000);
        assert!(!filter.matches(&event));
    }

    #[test]
    fn test_filter_matches_empty() {
        let filter = Filter::default();
        let event = make_event("id1", "author1", 1, vec![], 1000);
        assert!(filter.matches(&event));
    }

    #[test]
    fn test_filter_is_valid_requires_narrowing() {
        let limits = Limits::default();
        let filter = Filter::default();
        assert!(!filter.is_valid(&limits));
    }

    #[test]
    fn test_filter_is_valid_with_narrowing() {
        let limits = Limits::default();
        let filter = Filter {
            kinds: Some(vec![13194]),
            authors: Some(vec!["author1".into()]),
            ..Default::default()
        };
        assert!(filter.is_valid(&limits));
    }

    #[test]
    fn test_filter_is_valid_ids_exceeds_limit() {
        let limits = Limits { max_filter_items: 100, ..Default::default() };
        let filter = Filter {
            ids: Some(vec!["id1".into(); 200]),
            ..Default::default()
        };
        assert!(!filter.is_valid(&limits));
    }

    #[test]
    fn test_filter_is_valid_authors_exceeds_limit() {
        let limits = Limits { max_filter_items: 100, ..Default::default() };
        let filter = Filter {
            authors: Some(vec!["author1".into(); 150]),
            ..Default::default()
        };
        assert!(!filter.is_valid(&limits));
    }

    #[test]
    fn test_filter_pubkeys_from_authors() {
        let filter = Filter {
            authors: Some(vec!["author1".into(), "author2".into()]),
            ..Default::default()
        };
        let keys = filter.pubkeys();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"author1".into()));
        assert!(keys.contains(&"author2".into()));
    }

    #[test]
    fn test_filter_pubkeys_from_p_tags() {
        let filter = Filter {
            p_tags: Some(vec!["pubkey1".into(), "pubkey2".into()]),
            ..Default::default()
        };
        let keys = filter.pubkeys();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_filter_pubkeys_from_both() {
        let filter = Filter {
            authors: Some(vec!["author1".into()]),
            p_tags: Some(vec!["pubkey1".into()]),
            ..Default::default()
        };
        let keys = filter.pubkeys();
        assert_eq!(keys.len(), 2);
    }
}