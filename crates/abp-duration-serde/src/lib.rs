// SPDX-License-Identifier: MIT OR Apache-2.0
//! Serde helpers for serializing [`std::time::Duration`] values as milliseconds.

#![deny(unsafe_code)]
#![warn(missing_docs)]

/// Serde adapters for `Duration` <-> `u64` milliseconds.
pub mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    /// Serialize a [`Duration`] as whole milliseconds.
    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    /// Deserialize whole milliseconds into a [`Duration`].
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

/// Serde adapters for `Option<Duration>` <-> optional `u64` milliseconds.
pub mod option_duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    /// Serialize an optional [`Duration`] as optional whole milliseconds.
    pub fn serialize<S: Serializer>(
        duration: &Option<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match duration {
            Some(duration) => serializer.serialize_some(&(duration.as_millis() as u64)),
            None => serializer.serialize_none(),
        }
    }

    /// Deserialize optional whole milliseconds into an optional [`Duration`].
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Duration>, D::Error> {
        let millis: Option<u64> = Option::deserialize(deserializer)?;
        Ok(millis.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use super::{duration_millis, option_duration_millis};
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct DurationMs {
        #[serde(with = "duration_millis")]
        value: Duration,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct OptionDurationMs {
        #[serde(with = "option_duration_millis")]
        value: Option<Duration>,
    }

    #[test]
    fn duration_round_trip() {
        let sample = DurationMs {
            value: Duration::from_millis(750),
        };
        let encoded = serde_json::to_string(&sample).expect("serialize duration");
        assert_eq!(encoded, r#"{"value":750}"#);

        let decoded: DurationMs = serde_json::from_str(&encoded).expect("deserialize duration");
        assert_eq!(decoded, sample);
    }

    #[test]
    fn option_duration_round_trip_some_and_none() {
        let some = OptionDurationMs {
            value: Some(Duration::from_millis(250)),
        };
        let some_encoded = serde_json::to_string(&some).expect("serialize some duration");
        assert_eq!(some_encoded, r#"{"value":250}"#);

        let some_decoded: OptionDurationMs =
            serde_json::from_str(&some_encoded).expect("deserialize some duration");
        assert_eq!(some_decoded, some);

        let none = OptionDurationMs { value: None };
        let none_encoded = serde_json::to_string(&none).expect("serialize none duration");
        assert_eq!(none_encoded, r#"{"value":null}"#);

        let none_decoded: OptionDurationMs =
            serde_json::from_str(&none_encoded).expect("deserialize none duration");
        assert_eq!(none_decoded, none);
    }
}
