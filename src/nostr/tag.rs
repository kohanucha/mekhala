use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    P(String, Vec<String>),
    E(String, Vec<String>),
    Encryption(String),
    Expiration(u64),
    Other(String, Vec<String>),
}

impl Tag {
    pub fn p(pubkey: impl Into<String>) -> Self {
        Tag::P(pubkey.into(), vec![])
    }

    #[cfg(test)]
    pub fn e(event_id: impl Into<String>) -> Self {
        Tag::E(event_id.into(), vec![])
    }

    pub fn encryption(scheme: impl Into<String>) -> Self {
        Tag::Encryption(scheme.into())
    }

    pub fn expiration(ts: u64) -> Self {
        Tag::Expiration(ts)
    }

    pub fn is_p(&self) -> bool {
        matches!(self, Tag::P(_, _))
    }

    pub fn is_e(&self) -> bool {
        matches!(self, Tag::E(_, _))
    }

    pub fn pubkey(&self) -> Option<&str> {
        match self {
            Tag::P(pk, _) => Some(pk),
            _ => None,
        }
    }

    pub fn event_id(&self) -> Option<&str> {
        match self {
            Tag::E(id, _) => Some(id),
            _ => None,
        }
    }

    pub fn encryption_scheme(&self) -> Option<&str> {
        match self {
            Tag::Encryption(scheme) => Some(scheme),
            _ => None,
        }
    }
}

impl Serialize for Tag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let arr: Vec<Value> = match self {
            Tag::P(pk, extras) => {
                let mut v = vec![Value::String("p".into()), Value::String(pk.clone())];
                for e in extras {
                    v.push(Value::String(e.clone()));
                }
                v
            }
            Tag::E(id, extras) => {
                let mut v = vec![Value::String("e".into()), Value::String(id.clone())];
                for e in extras {
                    v.push(Value::String(e.clone()));
                }
                v
            }
            Tag::Encryption(scheme) => {
                vec![Value::String("encryption".into()), Value::String(scheme.clone())]
            }
            Tag::Expiration(ts) => {
                vec![Value::String("expiration".into()), Value::String(ts.to_string())]
            }
            Tag::Other(name, values) => {
                let mut v = vec![Value::String(name.clone())];
                for val in values {
                    v.push(Value::String(val.clone()));
                }
                v
            }
        };
        arr.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Tag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let arr: Vec<Value> = Vec::deserialize(deserializer)?;

        if arr.is_empty() {
            return Err(serde::de::Error::custom("empty tag array"));
        }

        let name = arr[0].as_str().ok_or_else(|| serde::de::Error::custom("tag name must be a string"))?;

        match name {
            "p" => {
                let pk = arr.get(1)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| serde::de::Error::custom("p-tag requires a pubkey value"))?
                    .to_string();
                let extras = arr.get(2..)
                    .map(|slice| slice.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                Ok(Tag::P(pk, extras))
            }
            "e" => {
                let id = arr.get(1)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| serde::de::Error::custom("e-tag requires an event id value"))?
                    .to_string();
                let extras = arr.get(2..)
                    .map(|slice| slice.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                Ok(Tag::E(id, extras))
            }
            "encryption" => {
                let scheme = arr.get(1)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| serde::de::Error::custom("encryption-tag requires a scheme value"))?
                    .to_string();
                Ok(Tag::Encryption(scheme))
            }
            "expiration" => {
                let ts_str = arr.get(1)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| serde::de::Error::custom("expiration-tag requires a timestamp value"))?;
                let ts: u64 = ts_str.parse()
                    .map_err(|_| serde::de::Error::custom("expiration-tag timestamp must be a number"))?;
                Ok(Tag::Expiration(ts))
            }
            _ => {
                let values: Vec<String> = arr.get(1..)
                    .map(|slice| slice.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                Ok(Tag::Other(name.to_string(), values))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_p_roundtrip() {
        let tag = Tag::p("abc123");
        let json = serde_json::to_value(&tag).unwrap();
        assert_eq!(json, serde_json::json!(["p", "abc123"]));

        let deserialized: Tag = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, Tag::P("abc123".into(), vec![]));
    }

    #[test]
    fn test_tag_p_with_extras_roundtrip() {
        let tag = Tag::P("abc123".into(), vec!["wss://relay.example.com".into(), "petname".into()]);
        let json = serde_json::to_value(&tag).unwrap();
        assert_eq!(json, serde_json::json!(["p", "abc123", "wss://relay.example.com", "petname"]));

        let deserialized: Tag = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, Tag::P("abc123".into(), vec!["wss://relay.example.com".into(), "petname".into()]));
    }

    #[test]
    fn test_tag_e_roundtrip() {
        let tag = Tag::e("event_id_1");
        let json = serde_json::to_value(&tag).unwrap();
        assert_eq!(json, serde_json::json!(["e", "event_id_1"]));

        let deserialized: Tag = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, Tag::E("event_id_1".into(), vec![]));
    }

    #[test]
    fn test_tag_encryption_roundtrip() {
        let tag = Tag::encryption("nip44_v2 nip04");
        let json = serde_json::to_value(&tag).unwrap();
        assert_eq!(json, serde_json::json!(["encryption", "nip44_v2 nip04"]));

        let deserialized: Tag = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, Tag::Encryption("nip44_v2 nip04".into()));
    }

    #[test]
    fn test_tag_expiration_roundtrip() {
        let tag = Tag::expiration(1234567890);
        let json = serde_json::to_value(&tag).unwrap();
        assert_eq!(json, serde_json::json!(["expiration", "1234567890"]));

        let deserialized: Tag = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, Tag::Expiration(1234567890));
    }

    #[test]
    fn test_tag_other_roundtrip() {
        let tag = Tag::Other("custom".into(), vec!["value1".into(), "value2".into()]);
        let json = serde_json::to_value(&tag).unwrap();
        assert_eq!(json, serde_json::json!(["custom", "value1", "value2"]));

        let deserialized: Tag = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, Tag::Other("custom".into(), vec!["value1".into(), "value2".into()]));
    }

    #[test]
    fn test_tag_accessors() {
        let p = Tag::p("pk1");
        assert!(p.is_p());
        assert!(!p.is_e());
        assert_eq!(p.pubkey(), Some("pk1"));
        assert_eq!(p.event_id(), None);
        assert_eq!(p.encryption_scheme(), None);

        let e = Tag::e("eid1");
        assert!(!e.is_p());
        assert!(e.is_e());
        assert_eq!(e.event_id(), Some("eid1"));
        assert_eq!(e.pubkey(), None);

        let enc = Tag::encryption("nip04");
        assert_eq!(enc.encryption_scheme(), Some("nip04"));
        assert_eq!(enc.pubkey(), None);

        let exp = Tag::expiration(999);
        assert_eq!(exp.pubkey(), None);
        assert_eq!(exp.event_id(), None);
    }

    #[test]
    fn test_tags_in_event_json() {
        let event_json = r#"{
            "id": "abc",
            "pubkey": "pk1",
            "created_at": 1000,
            "kind": 23194,
            "tags": [["p", "wallet_pk"], ["e", "event1"], ["encryption", "nip44_v2 nip04"]],
            "content": "hello",
            "sig": "sig1"
        }"#;

        let event: crate::nostr::Event = serde_json::from_str(event_json).unwrap();
        assert_eq!(event.tags.len(), 3);
        assert_eq!(event.tags[0], Tag::P("wallet_pk".into(), vec![]));
        assert_eq!(event.tags[1], Tag::E("event1".into(), vec![]));
        assert_eq!(event.tags[2], Tag::Encryption("nip44_v2 nip04".into()));
    }
}