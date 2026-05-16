// SPDX-License-Identifier: MIT OR Apache-2.0
//! Validate [`crate::BackplaneConfig::port`].
//!
//! The TOML deserializer already restricts the value to a `u16`, so the only
//! remaining check is that the port is non-zero.

use super::Findings;
use crate::BackplaneConfig;

pub(crate) fn check(config: &BackplaneConfig) -> Findings {
    let mut findings = Findings::default();
    if let Some(p) = config.port
        && p == 0
    {
        findings
            .errors
            .push("port must be between 1 and 65535".into());
    }
    findings
}
