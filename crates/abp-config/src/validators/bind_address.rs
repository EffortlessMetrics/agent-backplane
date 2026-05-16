// SPDX-License-Identifier: MIT OR Apache-2.0
//! Validate [`crate::BackplaneConfig::bind_address`] as an IP address or
//! hostname.

use super::Findings;
use crate::{BackplaneConfig, is_valid_hostname};

pub(crate) fn check(config: &BackplaneConfig) -> Findings {
    let mut findings = Findings::default();
    let Some(ref addr) = config.bind_address else {
        return findings;
    };

    if addr.trim().is_empty() {
        findings
            .errors
            .push("bind_address must not be empty".into());
    } else if addr.parse::<std::net::IpAddr>().is_err() && !is_valid_hostname(addr) {
        findings.errors.push(format!(
            "bind_address '{addr}' is not a valid IP address or hostname"
        ));
    }
    findings
}
