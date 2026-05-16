// SPDX-License-Identifier: MIT OR Apache-2.0
//! Emit advisory [`ConfigWarning::MissingOptionalField`] hints for fields that
//! are valuable to set but not strictly required.

use super::Findings;
use crate::{BackplaneConfig, ConfigWarning};

pub(crate) fn check(config: &BackplaneConfig) -> Findings {
    let mut findings = Findings::default();
    if config.default_backend.is_none() {
        findings.warnings.push(ConfigWarning::MissingOptionalField {
            field: "default_backend".into(),
            hint: "callers must always specify --backend explicitly".into(),
        });
    }
    if config.receipts_dir.is_none() {
        findings.warnings.push(ConfigWarning::MissingOptionalField {
            field: "receipts_dir".into(),
            hint: "receipts will not be persisted to disk".into(),
        });
    }
    findings
}
