// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

/// Serde helper for `Duration` as integer milliseconds.
pub mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    /// Serialize a [`Duration`] as integer milliseconds.
    pub fn serialize<S: Serializer>(val: &Duration, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_u64(val.as_millis() as u64)
    }

    /// Deserialize a [`Duration`] from integer milliseconds.
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(de)?;
        Ok(Duration::from_millis(ms))
    }
}

/// Serde helper for `Option<Duration>` as optional integer milliseconds.
pub mod option_duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    /// Serialize an optional [`Duration`] as optional integer milliseconds.
    pub fn serialize<S: Serializer>(val: &Option<Duration>, ser: S) -> Result<S::Ok, S::Error> {
        match val {
            Some(d) => ser.serialize_some(&(d.as_millis() as u64)),
            None => ser.serialize_none(),
        }
    }

    /// Deserialize an optional [`Duration`] from optional integer milliseconds.
    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Duration>, D::Error> {
        let opt: Option<u64> = Option::deserialize(de)?;
        Ok(opt.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct DurationOnly {
        #[serde(with = "crate::duration_millis")]
        value: Duration,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct OptionalDuration {
        #[serde(default, with = "crate::option_duration_millis")]
        value: Option<Duration>,
    }

    #[test]
    fn duration_roundtrip_uses_millis() {
        let src = DurationOnly {
            value: Duration::from_millis(1500),
        };
        let json = serde_json::to_string(&src).expect("serialize");
        assert_eq!(json, r#"{"value":1500}"#);

        let decoded: DurationOnly = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, src);
    }

    #[test]
    fn option_duration_roundtrip_none() {
        let src = OptionalDuration { value: None };
        let json = serde_json::to_string(&src).expect("serialize");
        assert_eq!(json, r#"{"value":null}"#);

        let decoded: OptionalDuration = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, src);
    }
}
