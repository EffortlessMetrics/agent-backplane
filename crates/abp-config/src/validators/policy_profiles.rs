// SPDX-License-Identifier: MIT OR Apache-2.0
//! Validate [`crate::BackplaneConfig::policy_profiles`] entries.
//!
//! Each entry must be a non-empty path that exists on disk. The existence
//! check uses [`std::path::Path::exists`], so symlinks are followed.

use super::Findings;
use crate::BackplaneConfig;
use std::path::Path;

pub(crate) fn check(config: &BackplaneConfig) -> Findings {
    let mut findings = Findings::default();
    for path_str in &config.policy_profiles {
        if path_str.trim().is_empty() {
            findings
                .errors
                .push("policy profile path must not be empty".into());
        } else if !Path::new(path_str).exists() {
            findings
                .errors
                .push(format!("policy profile path does not exist: {path_str}"));
        }
    }
    findings
}
