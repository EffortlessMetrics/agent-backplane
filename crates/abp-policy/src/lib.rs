//! abp-policy
//!
//! Policy evaluation.
//!
//! In v0.1 this is a small “contract utility” crate.
//! Eventually this becomes the opinionated policy engine.

use abp_core::PolicyProfile;
use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Decision {
    pub allowed: bool,
    pub reason: Option<String>,
}

impl Decision {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reason: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PolicyEngine {
    allowed_tools: Option<GlobSet>,
    disallowed_tools: Option<GlobSet>,
    deny_read: Option<GlobSet>,
    deny_write: Option<GlobSet>,
}

impl PolicyEngine {
    pub fn new(policy: &PolicyProfile) -> Result<Self> {
        Ok(Self {
            allowed_tools: build_globset(&policy.allowed_tools)?,
            disallowed_tools: build_globset(&policy.disallowed_tools)?,
            deny_read: build_globset(&policy.deny_read)?,
            deny_write: build_globset(&policy.deny_write)?,
        })
    }

    pub fn can_use_tool(&self, tool_name: &str) -> Decision {
        if let Some(deny) = &self.disallowed_tools {
            if deny.is_match(tool_name) {
                return Decision::deny(format!("tool '{tool_name}' is disallowed"));
            }
        }

        if let Some(allow) = &self.allowed_tools {
            if !allow.is_match(tool_name) {
                return Decision::deny(format!("tool '{tool_name}' not in allowlist"));
            }
        }

        Decision::allow()
    }

    pub fn can_read_path(&self, rel_path: &Path) -> Decision {
        let s = rel_path.to_string_lossy();
        if let Some(deny) = &self.deny_read {
            if deny.is_match(s.as_ref()) {
                return Decision::deny(format!("read denied for '{s}'"));
            }
        }
        Decision::allow()
    }

    pub fn can_write_path(&self, rel_path: &Path) -> Decision {
        let s = rel_path.to_string_lossy();
        if let Some(deny) = &self.deny_write {
            if deny.is_match(s.as_ref()) {
                return Decision::deny(format!("write denied for '{s}'"));
            }
        }
        Decision::allow()
    }
}

fn build_globset(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p).with_context(|| format!("invalid glob: {p}"))?);
    }
    Ok(Some(b.build()?))
}
