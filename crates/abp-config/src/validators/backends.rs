// SPDX-License-Identifier: MIT OR Apache-2.0
//! Validate every entry in [`crate::BackplaneConfig::backends`].
//!
//! Sidecar entries are checked for a non-empty command and an in-range
//! `timeout_secs`. Timeouts above [`crate::LARGE_TIMEOUT_THRESHOLD`] but still
//! within [`crate::MAX_TIMEOUT_SECS`] surface a
//! [`ConfigWarning::LargeTimeout`] rather than an error.

use super::Findings;
use crate::{
    BackendEntry, BackplaneConfig, ConfigWarning, LARGE_TIMEOUT_THRESHOLD, MAX_TIMEOUT_SECS,
};

pub(crate) fn check(config: &BackplaneConfig) -> Findings {
    let mut findings = Findings::default();
    for (name, backend) in &config.backends {
        check_entry(name, backend, &mut findings);
    }
    findings
}

fn check_entry(name: &str, backend: &BackendEntry, findings: &mut Findings) {
    if name.is_empty() {
        findings
            .errors
            .push("backend name must not be empty".into());
    }
    match backend {
        BackendEntry::Sidecar {
            command,
            timeout_secs,
            ..
        } => {
            check_sidecar_command(name, command, findings);
            if let Some(t) = timeout_secs {
                check_sidecar_timeout(name, *t, findings);
            }
        }
        BackendEntry::Mock {} => {}
    }
}

fn check_sidecar_command(name: &str, command: &str, findings: &mut Findings) {
    if command.trim().is_empty() {
        findings.errors.push(format!(
            "backend '{name}': sidecar command must not be empty"
        ));
    }
}

fn check_sidecar_timeout(name: &str, secs: u64, findings: &mut Findings) {
    if secs == 0 || secs > MAX_TIMEOUT_SECS {
        findings.errors.push(format!(
            "backend '{name}': timeout {secs}s out of range (1..{MAX_TIMEOUT_SECS})"
        ));
    } else if secs > LARGE_TIMEOUT_THRESHOLD {
        findings.warnings.push(ConfigWarning::LargeTimeout {
            backend: name.to_string(),
            secs,
        });
    }
}
