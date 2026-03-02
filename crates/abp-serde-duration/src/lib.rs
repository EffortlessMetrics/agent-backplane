// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shared serde adapters for `std::time::Duration`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::Duration;

/// Serde helpers for encoding `Duration` values as integer milliseconds.
pub mod duration_millis {
    use super::*;

    /// Serialize a duration as whole milliseconds (`u64`).
    pub fn serialize<S: Serializer>(val: &Duration, ser: S) -> Result<S::Ok, S::Error> {
        val.as_millis().serialize(ser)
    }

    /// Deserialize a duration from whole milliseconds (`u64`).
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Duration, D::Error> {
        let ms: u64 = u64::deserialize(de)?;
        Ok(Duration::from_millis(ms))
    }
}

/// Serde helpers for encoding `Option<Duration>` values as integer milliseconds.
pub mod option_duration_millis {
    use super::*;

    /// Serialize an optional duration as optional whole milliseconds (`Option<u64>`).
    pub fn serialize<S: Serializer>(val: &Option<Duration>, ser: S) -> Result<S::Ok, S::Error> {
        match val {
            Some(d) => d.as_millis().serialize(ser),
            None => ser.serialize_none(),
        }
    }

    /// Deserialize an optional duration from optional whole milliseconds (`Option<u64>`).
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Duration>, D::Error> {
        let opt: Option<u64> = Option::deserialize(de)?;
        Ok(opt.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct WithDuration {
        #[serde(with = "crate::duration_millis")]
        value: Duration,
    }

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct WithOptionDuration {
        #[serde(with = "crate::option_duration_millis")]
        value: Option<Duration>,
    }

    #[test]
    fn duration_roundtrip_uses_milliseconds() {
        let payload = WithDuration {
            value: Duration::from_millis(1250),
        };

        let json = serde_json::to_string(&payload).expect("serialize");
        assert_eq!(json, r#"{"value":1250}"#);

        let decoded: WithDuration = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn option_duration_roundtrip_handles_some_and_none() {
        let some = WithOptionDuration {
            value: Some(Duration::from_millis(33)),
        };
        let none = WithOptionDuration { value: None };

        let some_json = serde_json::to_string(&some).expect("serialize some");
        let none_json = serde_json::to_string(&none).expect("serialize none");

        assert_eq!(some_json, r#"{"value":33}"#);
        assert_eq!(none_json, r#"{"value":null}"#);

        let some_decoded: WithOptionDuration =
            serde_json::from_str(&some_json).expect("deserialize some");
        let none_decoded: WithOptionDuration =
            serde_json::from_str(&none_json).expect("deserialize none");

        assert_eq!(some_decoded, some);
        assert_eq!(none_decoded, none);
    }
}
