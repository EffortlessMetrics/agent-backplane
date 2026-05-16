// SPDX-License-Identifier: MIT OR Apache-2.0
//! Validate [`crate::BackplaneConfig::log_level`] against [`crate::VALID_LOG_LEVELS`].

use super::Findings;
use crate::{BackplaneConfig, VALID_LOG_LEVELS};

pub(crate) fn check(config: &BackplaneConfig) -> Findings {
    let mut findings = Findings::default();
    if let Some(ref level) = config.log_level
        && !VALID_LOG_LEVELS.contains(&level.as_str())
    {
        findings.errors.push(format!("invalid log_level '{level}'"));
    }
    findings
}
