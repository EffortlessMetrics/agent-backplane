// SPDX-License-Identifier: MIT OR Apache-2.0
//! Serde adapters for `std::time::Duration` values represented as milliseconds.

use std::time::Duration;

/// Serde helper module for a required `Duration` represented as `u64` milliseconds.
pub mod duration_millis {
    use super::Duration;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize a `Duration` into `u64` milliseconds.
    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        let millis = u64::try_from(duration.as_millis())
            .map_err(|_| serde::ser::Error::custom("duration millis exceeds u64"))?;
        serializer.serialize_u64(millis)
    }

    /// Deserialize a `Duration` from `u64` milliseconds.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(ms))
    }
}

/// Serde helper module for an optional `Duration` represented as optional `u64` milliseconds.
pub mod option_duration_millis {
    use super::Duration;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize an optional `Duration` into optional `u64` milliseconds.
    pub fn serialize<S: Serializer>(
        value: &Option<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(duration) => {
                let millis = u64::try_from(duration.as_millis())
                    .map_err(|_| serde::ser::Error::custom("duration millis exceeds u64"))?;
                serializer.serialize_some(&millis)
            }
            None => serializer.serialize_none(),
        }
    }

    /// Deserialize an optional `Duration` from optional `u64` milliseconds.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Duration>, D::Error> {
        let ms: Option<u64> = Option::deserialize(deserializer)?;
        Ok(ms.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use super::{duration_millis, option_duration_millis};
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct RequiredDuration {
        #[serde(with = "duration_millis")]
        duration: Duration,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct OptionalDuration {
        #[serde(with = "option_duration_millis")]
        duration: Option<Duration>,
    }

    #[test]
    fn roundtrips_required_duration() {
        let value = RequiredDuration {
            duration: Duration::from_millis(1500),
        };

        let json = serde_json::to_string(&value).expect("serialize duration");
        assert_eq!(json, r#"{"duration":1500}"#);

        let parsed: RequiredDuration = serde_json::from_str(&json).expect("deserialize duration");
        assert_eq!(parsed, value);
    }

    #[test]
    fn roundtrips_optional_duration_some() {
        let value = OptionalDuration {
            duration: Some(Duration::from_millis(42)),
        };

        let json = serde_json::to_string(&value).expect("serialize optional duration");
        assert_eq!(json, r#"{"duration":42}"#);

        let parsed: OptionalDuration =
            serde_json::from_str(&json).expect("deserialize optional duration");
        assert_eq!(parsed, value);
    }

    #[test]
    fn roundtrips_optional_duration_none() {
        let value = OptionalDuration { duration: None };

        let json = serde_json::to_string(&value).expect("serialize optional none");
        assert_eq!(json, r#"{"duration":null}"#);

        let parsed: OptionalDuration =
            serde_json::from_str(&json).expect("deserialize optional none");
        assert_eq!(parsed, value);
    }
}
