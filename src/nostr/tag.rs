use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// A NIP-01 event tag. `p` and `e` are typed because routing reads them;
/// every other tag is kept as received.
///
/// The event id is recomputed from these tags, so each variant must serialize
/// back exactly as the client signed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    P(String, Vec<Value>),
    E(String, Vec<Value>),
    Other(String, Vec<Value>),
}

impl Tag {
    /// Whether this is a `p` (recipient public key) tag.
    pub fn is_p(&self) -> bool {
        matches!(self, Self::P(_, _))
    }

    /// Whether this is an `e` (referenced event ID) tag.
    pub fn is_e(&self) -> bool {
        matches!(self, Self::E(_, _))
    }

    /// The public key if this is a `p` tag.
    pub fn pubkey(&self) -> Option<&str> {
        match self {
            Self::P(pk, _) => Some(pk),
            _ => None,
        }
    }

    /// The referenced event ID if this is an `e` tag.
    pub fn event_id(&self) -> Option<&str> {
        match self {
            Self::E(id, _) => Some(id),
            _ => None,
        }
    }

    /// The kind number if this is a `k` tag (accepts numeric or string
    /// forms).
    pub fn kind_value(&self) -> Option<u64> {
        match self {
            Self::Other(name, values) if name == "k" => values.first().and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
            }),
            _ => None,
        }
    }
}

impl Serialize for Tag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let arr: Vec<Value> = match self {
            Self::P(pk, extras) => {
                let mut v = vec![Value::String("p".into()), Value::String(pk.clone())];
                v.extend(extras.iter().cloned());
                v
            }
            Self::E(id, extras) => {
                let mut v = vec![Value::String("e".into()), Value::String(id.clone())];
                v.extend(extras.iter().cloned());
                v
            }
            Self::Other(name, values) => {
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

        let name = arr[0]
            .as_str()
            .ok_or_else(|| serde::de::Error::custom("tag name must be a string"))?;

        match name {
            "p" => {
                let pk = arr
                    .get(1)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| serde::de::Error::custom("p-tag requires a pubkey value"))?
                    .to_string();
                let extras = arr.get(2..).unwrap_or(&[]).to_vec();
                Ok(Self::P(pk, extras))
            }
            "e" => {
                let id = arr
                    .get(1)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| serde::de::Error::custom("e-tag requires an event id value"))?
                    .to_string();
                let extras = arr.get(2..).unwrap_or(&[]).to_vec();
                Ok(Self::E(id, extras))
            }
            _ => {
                let values = arr.get(1..).unwrap_or(&[]).to_vec();
                Ok(Self::Other(name.to_string(), values))
            }
        }
    }
}

#[cfg(test)]
#[path = "tag_test.rs"]
mod tag_test;
