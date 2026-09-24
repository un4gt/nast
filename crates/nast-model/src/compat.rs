//! Small, explicit wire compatibility helpers; invalid values remain errors.
use serde::{Deserialize, Deserializer};

pub fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where D: Deserializer<'de>, T: Deserialize<'de> + Default {
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

pub fn string_or_number<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where D: Deserializer<'de> {
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s)),
        Some(serde_json::Value::Number(n)) => Ok(Some(n.to_string())),
        _ => Err(serde::de::Error::custom("expected a string, number, or null")),
    }
}

pub fn timestamp<'de, D>(deserializer: D) -> Result<String, D::Error>
where D: Deserializer<'de> {
    Ok(string_or_number(deserializer)?.unwrap_or_default())
}

pub fn prompt_role<'de, D>(deserializer: D) -> Result<String, D::Error>
where D: Deserializer<'de> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(0) => Ok("system".into()), Some(1) => Ok("user".into()), Some(2) => Ok("assistant".into()),
            _ => Err(serde::de::Error::custom("invalid prompt role")),
        },
        serde_json::Value::String(s) if ["system","user","assistant"].contains(&s.as_str()) => Ok(s),
        _ => Err(serde::de::Error::custom("invalid prompt role")),
    }
}

pub fn serialize_prompt_role<S: serde::Serializer>(role: &str, serializer: S) -> Result<S::Ok,S::Error> {
    serializer.serialize_i64(match role { "user"=>1,"assistant"=>2,_=>0 })
}
