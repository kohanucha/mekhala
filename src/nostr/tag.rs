use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    P(String, Vec<Value>),
    E(String, Vec<Value>),
    Encryption(String),
    Expiration(String),
    Other(String, Vec<Value>),
}

impl Tag {
    pub fn p(pubkey: impl Into<String>) -> Self {
        Tag::P(pubkey.into(), vec![])
    }

    pub fn encryption(scheme: impl Into<String>) -> Self {
        Tag::Encryption(scheme.into())
    }

    pub fn expiration(ts: u64) -> Self {
        Tag::Expiration(ts.to_string())
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