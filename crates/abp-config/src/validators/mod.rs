// SPDX-License-Identifier: MIT OR Apache-2.0
//! Single-responsibility validators backing [`crate::validate_config`].
//!
//! Each submodule inspects one logical slice of [`crate::BackplaneConfig`]
//! (log level, port, bind address, policy profiles, backends, advisory hints)
//! and returns the [`Findings`] it discovered. The top-level orchestrator
//! [`run_all`] composes them into the combined result the public
//! `validate_config` function returns.

use crate::ConfigWarning;

pub(crate) mod advisory;
pub(crate) mod backends;
pub(crate) mod bind_address;
pub(crate) mod log_level;
pub(crate) mod policy_profiles;
pub(crate) mod port;

/// Accumulator returned by each focused validator.
///
/// Validators only describe what they found; deciding whether the overall
/// outcome is `Ok` or `Err` is the orchestrator's job.
#[derive(Debug, Default)]
pub(crate) struct Findings {
    /// Hard validation errors. A non-empty list converts the result into a
    /// [`crate::ConfigError::ValidationError`].
    pub errors: Vec<String>,
    /// Advisory warnings surfaced to the caller alongside a successful result.
    pub warnings: Vec<ConfigWarning>,
}

impl Findings {
    /// Absorb another validator's findings, preserving append order.
    pub(crate) fn extend(&mut self, other: Findings) {
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
    }
}

/// Run every focused validator against `config` and return the combined
/// findings.
///
/// The call order is the same order as the original monolithic
/// `validate_config` to keep error/warning sequences stable for callers that
/// surface them to humans.
pub(crate) fn run_all(config: &crate::BackplaneConfig) -> Findings {
    let mut findings = Findings::default();
    findings.extend(log_level::check(config));
    findings.extend(port::check(config));
    findings.extend(bind_address::check(config));
    findings.extend(policy_profiles::check(config));
    findings.extend(backends::check(config));
    findings.extend(advisory::check(config));
    findings
}
