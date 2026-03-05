// SPDX-License-Identifier: MIT OR Apache-2.0
//! Serde adapters for `std::time::Duration` values.

/// Serialize/deserialize `Duration` as integer milliseconds.
pub mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

/// Serialize/deserialize `Option<Duration>` as optional integer milliseconds.
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
    fn duration_round_trip() {
        let value = WithDuration {
            value: Duration::from_millis(250),
        };

        let json = serde_json::to_string(&value).expect("serialize duration");
        assert_eq!(json, r#"{"value":250}"#);

        let decoded: WithDuration = serde_json::from_str(&json).expect("deserialize duration");
        assert_eq!(decoded, value);
    }

    #[test]
    fn optional_duration_round_trip_some_and_none() {
        let some = WithOptionalDuration {
            value: Some(Duration::from_millis(10)),
        };
        let none = WithOptionalDuration { value: None };

        let some_json = serde_json::to_string(&some).expect("serialize some");
        let none_json = serde_json::to_string(&none).expect("serialize none");

        assert_eq!(some_json, r#"{"value":10}"#);
        assert_eq!(none_json, r#"{"value":null}"#);

        let some_decoded: WithOptionalDuration =
            serde_json::from_str(&some_json).expect("deserialize some");
        let none_decoded: WithOptionalDuration =
            serde_json::from_str(&none_json).expect("deserialize none");

        assert_eq!(some_decoded, some);
        assert_eq!(none_decoded, none);
    }
}
