// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shared serde adapters.

/// Serialize/deserialize [`std::time::Duration`] as milliseconds.
pub mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(ms))
    }
}

/// Serialize/deserialize [`Option<std::time::Duration>`] as optional milliseconds.
pub mod option_duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(
        value: &Option<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(duration) => serializer.serialize_some(&(duration.as_millis() as u64)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Duration>, D::Error> {
        let ms: Option<u64> = Option::deserialize(deserializer)?;
        Ok(ms.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct DurationHolder {
        #[serde(with = "duration_millis")]
        value: Duration,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct OptionDurationHolder {
        #[serde(with = "option_duration_millis")]
        value: Option<Duration>,
    }

    #[test]
    fn duration_roundtrip() {
        let original = DurationHolder {
            value: Duration::from_millis(250),
        };

        let json = serde_json::to_string(&original).expect("serialize");
        assert_eq!(json, r#"{"value":250}"#);

        let parsed: DurationHolder = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, original);
    }

    #[test]
    fn option_duration_roundtrip_some() {
        let original = OptionDurationHolder {
            value: Some(Duration::from_millis(500)),
        };

        let json = serde_json::to_string(&original).expect("serialize");
        assert_eq!(json, r#"{"value":500}"#);

        let parsed: OptionDurationHolder = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, original);
    }

    #[test]
    fn option_duration_roundtrip_none() {
        let original = OptionDurationHolder { value: None };

        let json = serde_json::to_string(&original).expect("serialize");
        assert_eq!(json, r#"{"value":null}"#);

        let parsed: OptionDurationHolder = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, original);
    }
}
