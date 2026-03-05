// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shared serde adapters used across ABP crates.

/// Serialize/deserialize [`std::time::Duration`] as integer milliseconds.
pub mod duration_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(val: &Duration, ser: S) -> Result<S::Ok, S::Error> {
        val.as_millis().serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Duration, D::Error> {
        let ms: u64 = u64::deserialize(de)?;
        Ok(Duration::from_millis(ms))
    }
}

/// Serialize/deserialize `Option<Duration>` as optional integer milliseconds.
pub mod option_duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(val: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match val {
            Some(d) => s.serialize_some(&(d.as_millis() as u64)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        let opt: Option<u64> = Option::deserialize(d)?;
        Ok(opt.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct DurWrap {
        #[serde(with = "crate::duration_millis")]
        d: Duration,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct OptWrap {
        #[serde(with = "crate::option_duration_millis")]
        d: Option<Duration>,
    }

    #[test]
    fn duration_roundtrip() {
        let wrapped = DurWrap {
            d: Duration::from_millis(321),
        };
        let v = serde_json::to_value(&wrapped).expect("serialize duration wrapper");
        assert_eq!(v, serde_json::json!({ "d": 321 }));

        let parsed: DurWrap = serde_json::from_value(v).expect("deserialize duration wrapper");
        assert_eq!(parsed, wrapped);
    }

    #[test]
    fn option_duration_roundtrip() {
        let wrapped = OptWrap {
            d: Some(Duration::from_millis(654)),
        };
        let v = serde_json::to_value(&wrapped).expect("serialize option duration wrapper");
        assert_eq!(v, serde_json::json!({ "d": 654 }));

        let parsed: OptWrap = serde_json::from_value(v).expect("deserialize option duration");
        assert_eq!(parsed, wrapped);

        let none_v = serde_json::json!({"d": null});
        let parsed_none: OptWrap =
            serde_json::from_value(none_v).expect("deserialize null option duration");
        assert_eq!(parsed_none, OptWrap { d: None });
    }
}
