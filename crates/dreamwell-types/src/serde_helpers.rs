use serde::{Deserialize, Deserializer, Serialize};

/// Accepts JSON strings, numbers, booleans, or null for optional text state values.
pub fn deserialize_optional_literal_string<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        Some(serde_json::Value::Bool(b)) => Some(b.to_string()),
        Some(other) => Some(other.to_string()),
    })
}

/// Accepts either a comma-separated string or a JSON array of strings.
pub fn deserialize_string_or_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(s) => s
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect(),
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    })
}

pub fn serialize_optional_literal_string<S>(
    value: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(v) => v.serialize(serializer),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct ValueField {
        #[serde(default, deserialize_with = "deserialize_optional_literal_string")]
        value: Option<String>,
    }

    #[test]
    fn deserializes_string_value() {
        let parsed: ValueField = serde_json::from_str(r#"{"value":"green"}"#).unwrap();
        assert_eq!(parsed.value.as_deref(), Some("green"));
    }

    #[test]
    fn deserializes_unquoted_number_as_string() {
        let parsed: ValueField = serde_json::from_str(r#"{"value":42}"#).unwrap();
        assert_eq!(parsed.value.as_deref(), Some("42"));
    }

    #[test]
    fn deserializes_boolean_as_string() {
        let parsed: ValueField = serde_json::from_str(r#"{"value":true}"#).unwrap();
        assert_eq!(parsed.value.as_deref(), Some("true"));
    }

    #[test]
    fn deserializes_null_as_none() {
        let parsed: ValueField = serde_json::from_str(r#"{"value":null}"#).unwrap();
        assert_eq!(parsed.value, None);
    }

    #[test]
    fn missing_value_defaults_to_none() {
        let parsed: ValueField = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed.value, None);
    }
}
