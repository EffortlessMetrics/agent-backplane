// SPDX-License-Identifier: MIT OR Apache-2.0
//! Serde helpers for encoding [`std::time::Duration`] values as milliseconds.
#![deny(unsafe_code)]
#![warn(missing_docs)]

/// Serde helpers for `Duration` represented as a millisecond integer (`u64`).
pub mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    /// Serialize `Duration` to integer milliseconds.
    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    /// Deserialize `Duration` from integer milliseconds.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

/// Serde helpers for `Option<Duration>` represented as optional millisecond integers.
pub mod option_duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    /// Serialize `Option<Duration>` to optional integer milliseconds.
    pub fn serialize<S: Serializer>(
        duration: &Option<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match duration {
            Some(value) => serializer.serialize_some(&(value.as_millis() as u64)),
            None => serializer.serialize_none(),
        }
    }

    /// Deserialize `Option<Duration>` from optional integer milliseconds.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Duration>, D::Error> {
        let millis: Option<u64> = Option::deserialize(deserializer)?;
        Ok(millis.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct WithDuration {
        #[serde(with = "crate::duration_millis")]
        value: Duration,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct WithOptionalDuration {
        #[serde(with = "crate::option_duration_millis")]
        value: Option<Duration>,
    }

    #[test]
    fn duration_roundtrip_as_millis() {
        let input = WithDuration {
            value: Duration::from_millis(250),
        };
        let json = serde_json::to_string(&input).expect("serialize");
        assert_eq!(json, r#"{"value":250}"#);

        let output: WithDuration = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(output, input);
    }

    #[test]
    fn option_duration_roundtrip_some_and_none() {
        let some = WithOptionalDuration {
            value: Some(Duration::from_millis(42)),
        };
        let some_json = serde_json::to_string(&some).expect("serialize some");
        assert_eq!(some_json, r#"{"value":42}"#);
        let some_output: WithOptionalDuration =
            serde_json::from_str(&some_json).expect("deserialize some");
        assert_eq!(some_output, some);

        let none = WithOptionalDuration { value: None };
        let none_json = serde_json::to_string(&none).expect("serialize none");
        assert_eq!(none_json, r#"{"value":null}"#);
        let none_output: WithOptionalDuration =
            serde_json::from_str(&none_json).expect("deserialize none");
        assert_eq!(none_output, none);
    }
}
