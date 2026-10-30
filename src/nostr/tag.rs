use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// A NIP-01 event tag, decoded from its wire form into typed variants used
/// by relay validation and NIP-47 routing (`p`, `e`, `encryption`,
/// `expiration`, and everything else).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    P(String, Vec<Value>),
    E(String, Vec<Value>),
    Encryption(String),
    Expiration(String),
    Other(String, Vec<Value>),
}

impl Tag {
    /// Builds a `p` tag for the given recipient public key.
    pub fn p(pubkey: impl Into<String>) -> Self {
        Tag::P(pubkey.into(), vec![])
    }

    /// Builds an `encryption` tag carrying NIP-47 protocol strings.
    pub fn encryption(scheme: impl Into<String>) -> Self {
        Tag::Encryption(scheme.into())
    }

    /// Builds an `expiration` tag for the given Unix timestamp.
    pub fn expiration(ts: u64) -> Self {
        Tag::Expiration(ts.to_string())
    }

    /// Whether this is a `p` (recipient public key) tag.
    pub fn is_p(&self) -> bool {
        matches!(self, Tag::P(_, _))
    }

    /// Whether this is an `e` (referenced event ID) tag.
    pub fn is_e(&self) -> bool {
        matches!(self, Tag::E(_, _))
    }

    /// The public key if this is a `p` tag.
    pub fn pubkey(&self) -> Option<&str> {
        match self {
            Tag::P(pk, _) => Some(pk),
            _ => None,
        }
    }

    /// The referenced event ID if this is an `e` tag.
    pub fn event_id(&self) -> Option<&str> {
        match self {
            Tag::E(id, _) => Some(id),
            _ => None,
        }
    }

    /// The encryption scheme string if this is an `encryption` tag.
    pub fn encryption_scheme(&self) -> Option<&str> {
        match self {
            Tag::Encryption(scheme) => Some(scheme),
            _ => None,
        }
    }

    /// The kind number if this is a `k` tag (accepts numeric or string
    /// forms).
    pub fn kind_value(&self) -> Option<u64> {
        match self {
            Tag::Other(name, values) if name == "k" => {
                values.first().and_then(|v| {
                    v.as_u64().or_else(|| {
                        v.as_str().and_then(|s| s.parse::<u64>().ok())
                    })
                })
            }
            _ => None,
        }
    }
}

impl Serialize for Tag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let arr: Vec<Value> = match self {
            Tag::P(pk, extras) => {
                let mut v = vec![Value::String("p".into()), Value::String(pk.clone())];
                v.extend(extras.iter().cloned());
                v
            }
            Tag::E(id, extras) => {
                let mut v = vec![Value::String("e".into()), Value::String(id.clone())];
                v.extend(extras.iter().cloned());
                v
            }
            Tag::Encryption(scheme) => {
                vec![Value::String("encryption".into()), Value::String(scheme.clone())]
            }
            Tag::Expiration(ts) => {
                vec![Value::String("expiration".into()), Value::String(ts.clone())]
            }
            Tag::Other(name, values) => {
                let mut v = vec![Value::String(name.clone())];
                v.extend(values.iter().cloned());
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
                let extras = arr.get(2..).unwrap_or(&[]).to_vec();
                Ok(Tag::P(pk, extras))
            }
            "e" => {
                let id = arr.get(1)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| serde::de::Error::custom("e-tag requires an event id value"))?
                    .to_string();
                let extras = arr.get(2..).unwrap_or(&[]).to_vec();
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
                let ts = arr.get(1)
                    .ok_or_else(|| serde::de::Error::custom("expiration-tag requires a timestamp value"))?;
                let ts_str = match ts.as_str() {
                    Some(s) => s.to_string(),
                    None => ts.as_u64()
                        .ok_or_else(|| serde::de::Error::custom("expiration-tag timestamp must be a number or numeric string"))?
                        .to_string(),
                };
                let _: u64 = ts_str.parse()
                    .map_err(|_| serde::de::Error::custom("expiration-tag timestamp must be a number"))?;
                Ok(Tag::Expiration(ts_str))
            }
            _ => {
                let values = arr.get(1..).unwrap_or(&[]).to_vec();
                Ok(Tag::Other(name.to_string(), values))
            }
        }
    }
}

#[cfg(test)]
#[path = "tag_test.rs"]
mod tag_test;