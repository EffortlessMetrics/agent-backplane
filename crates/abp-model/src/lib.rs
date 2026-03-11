// SPDX-License-Identifier: MIT OR Apache-2.0
//! Canonical model identifier helpers for vendor SDK adapters.

/// Build a canonical model identifier by prefixing the vendor namespace.
#[must_use]
pub fn to_canonical_model(vendor: &str, vendor_model: &str) -> String {
    format!("{vendor}/{vendor_model}")
}

/// Strip a vendor namespace prefix from a canonical model identifier.
///
/// Returns the original string if it does not have the expected prefix.
#[must_use]
pub fn from_canonical_model(vendor: &str, canonical: &str) -> String {
    canonical
        .strip_prefix(&format!("{vendor}/"))
        .unwrap_or(canonical)
        .to_string()
}

/// Return `true` when `model` is in the known model set.
#[must_use]
pub fn is_known_model(model: &str, known_models: &[&str]) -> bool {
    known_models.contains(&model)
}

#[cfg(test)]
mod tests {
    use super::{from_canonical_model, is_known_model, to_canonical_model};

    #[test]
    fn canonical_round_trip() {
        let canonical = to_canonical_model("openai", "gpt-5");
        assert_eq!(canonical, "openai/gpt-5");
        assert_eq!(from_canonical_model("openai", &canonical), "gpt-5");
    }

    #[test]
    fn from_canonical_passthrough_on_vendor_mismatch() {
        assert_eq!(
            from_canonical_model("gemini", "openai/gpt-5"),
            "openai/gpt-5"
        );
    }

    #[test]
    fn known_model_lookup() {
        let known = ["a", "b"];
        assert!(is_known_model("a", &known));
        assert!(!is_known_model("x", &known));
    }
}
