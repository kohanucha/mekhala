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
                t.pubkey().map_or(false, |pk| p_tags.iter().any(|s| s == pk))
            });
            if !has_match {
                return false;
            }
        }
        if let Some(e_tags) = &self.e_tags {
            let has_match = event.tags.iter().any(|t| {
                t.event_id().map_or(false, |eid| e_tags.iter().any(|s| s == eid))
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
                if kinds.iter().any(|k| !matches!(k, 13194 | 23194..=23197)) {
                    return false;
                }
            }
        } else {
            let kinds = match &self.kinds {
                Some(k) if !k.is_empty() => k,
                _ => return false,
            };

            if kinds.iter().any(|k| !matches!(k, 13194 | 23194..=23197)) {
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
mod tests {
    use super::*;
    use crate::nostr::Tag;

    fn make_event(id: &str, pubkey: &str, kind: u64, tags: Vec<Tag>, created_at: u64) -> Event {
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
            Tag::p("pubkey1"),
        ], 1000);
        assert!(filter.matches(&event));
        
        let event = make_event("id1", "author1", 1, vec![
            Tag::p("pubkey2"),
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
            Tag::E("event1".into(), vec![]),
        ], 1000);
        assert!(filter.matches(&event));
        
        let event = make_event("id1", "author1", 1, vec![
            Tag::E("event2".into(), vec![]),
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
        let filter = Filter::default();
        assert!(!filter.is_valid());
    }

    #[test]
    fn test_filter_is_valid_with_narrowing() {
        let filter = Filter {
            kinds: Some(vec![13194]),
            p_tags: Some(vec!["author1".into()]),
            ..Default::default()
        };
        assert!(filter.is_valid());
    }

    #[test]
    fn test_filter_is_valid_requires_kinds() {
        let filter = Filter {
            authors: Some(vec!["author1".into()]),
            ..Default::default()
        };
        assert!(!filter.is_valid());
    }

    #[test]
    fn test_filter_is_valid_rejects_non_nwc_kinds() {
        let filter = Filter {
            kinds: Some(vec![1]),
            authors: Some(vec!["author1".into()]),
            ..Default::default()
        };
        assert!(!filter.is_valid());

        let filter = Filter {
            kinds: Some(vec![13194, 1]),
            authors: Some(vec!["author1".into()]),
            ..Default::default()
        };
        assert!(!filter.is_valid());
    }

    #[test]
    fn test_filter_is_valid_accepts_nwc_kinds() {
        for kind in [13194u64, 23194, 23195, 23196, 23197] {
            let filter = Filter {
                kinds: Some(vec![kind]),
                authors: Some(vec!["author1".into()]),
                ..Default::default()
            };
            assert!(filter.is_valid(), "kind {} should be valid", kind);
        }
    }

    #[test]
    fn test_filter_is_valid_with_p_tags_only() {
        let filter = Filter {
            p_tags: Some(vec!["pubkey1".into()]),
            ..Default::default()
        };
        assert!(filter.is_valid());
    }

    #[test]
    fn test_filter_limit_deserialization() {
        let json = r#"{"kinds": [23194], "limit": 1}"#;
        let filter: Filter = serde_json::from_str(json).unwrap();
        assert_eq!(filter.limit, Some(1));
        assert_eq!(filter.kinds, Some(vec![23194]));

        let json_no_limit = r#"{"kinds": [23194], "authors": ["pk1"]}"#;
        let filter_no_limit: Filter = serde_json::from_str(json_no_limit).unwrap();
        assert_eq!(filter_no_limit.limit, None);
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