//! abp-policy
//!
//! Policy evaluation.
//!
//! In v0.1 this is a small “contract utility” crate.
//! Eventually this becomes the opinionated policy engine.

use abp_core::PolicyProfile;
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use anyhow::{Context, Result};
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
    tool_rules: IncludeExcludeGlobs,
    deny_read: IncludeExcludeGlobs,
    deny_write: IncludeExcludeGlobs,
}

impl PolicyEngine {
    pub fn new(policy: &PolicyProfile) -> Result<Self> {
        let no_include: &[String] = &[];
        Ok(Self {
            tool_rules: IncludeExcludeGlobs::new(&policy.allowed_tools, &policy.disallowed_tools)
                .context("compile tool policy globs")?,
            deny_read: IncludeExcludeGlobs::new(no_include, &policy.deny_read)
                .context("compile deny_read globs")?,
            deny_write: IncludeExcludeGlobs::new(no_include, &policy.deny_write)
                .context("compile deny_write globs")?,
        })
    }

    pub fn can_use_tool(&self, tool_name: &str) -> Decision {
        match self.tool_rules.decide_str(tool_name) {
            MatchDecision::Allowed => Decision::allow(),
            MatchDecision::DeniedByExclude => {
                Decision::deny(format!("tool '{tool_name}' is disallowed"))
            }
            MatchDecision::DeniedByMissingInclude => {
                Decision::deny(format!("tool '{tool_name}' not in allowlist"))
            }
        }
    }

    pub fn can_read_path(&self, rel_path: &Path) -> Decision {
        let s = rel_path.to_string_lossy();
        if !self.deny_read.decide_path(rel_path).is_allowed() {
            return Decision::deny(format!("read denied for '{s}'"));
        }
        Decision::allow()
    }

    pub fn can_write_path(&self, rel_path: &Path) -> Decision {
        let s = rel_path.to_string_lossy();
        if !self.deny_write.decide_path(rel_path).is_allowed() {
            return Decision::deny(format!("write denied for '{s}'"));
        }
        Decision::allow()
    }
}

#[cfg(test)]
mod tests {
    use super::PolicyEngine;
    use abp_core::PolicyProfile;
    use std::path::Path;

    #[test]
    fn disallowed_tool_beats_allowlist() {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec!["Bash".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).expect("compile policy");

        let decision = engine.can_use_tool("Bash");
        assert!(!decision.allowed);
        assert_eq!(
            decision.reason.as_deref(),
            Some("tool 'Bash' is disallowed")
        );
    }

    #[test]
    fn allowlist_blocks_unlisted_tool() {
        let policy = PolicyProfile {
            allowed_tools: vec!["Read".to_string(), "Write".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).expect("compile policy");

        let denied = engine.can_use_tool("Bash");
        assert!(!denied.allowed);
        assert_eq!(
            denied.reason.as_deref(),
            Some("tool 'Bash' not in allowlist")
        );

        let allowed = engine.can_use_tool("Read");
        assert!(allowed.allowed);
    }

    #[test]
    fn deny_read_and_write_matchers_are_enforced() {
        let policy = PolicyProfile {
            deny_read: vec!["secret*".to_string()],
            deny_write: vec!["locked*".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).expect("compile policy");

        let read_denied = engine.can_read_path(Path::new("secret.txt"));
        assert!(!read_denied.allowed);

        let write_denied = engine.can_write_path(Path::new("locked.md"));
        assert!(!write_denied.allowed);

        let read_allowed = engine.can_read_path(Path::new("src/lib.rs"));
        assert!(read_allowed.allowed);
    }

    #[test]
    fn empty_policy_allows_everything() {
        let engine = PolicyEngine::new(&PolicyProfile::default()).expect("compile policy");

        assert!(engine.can_use_tool("Bash").allowed);
        assert!(engine.can_use_tool("Read").allowed);
        assert!(engine.can_use_tool("Write").allowed);
        assert!(engine.can_read_path(Path::new("any/file.txt")).allowed);
        assert!(engine.can_write_path(Path::new("any/file.txt")).allowed);
    }

    #[test]
    fn glob_patterns_in_tool_rules() {
        let policy = PolicyProfile {
            disallowed_tools: vec!["Bash*".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).expect("compile policy");

        assert!(!engine.can_use_tool("BashExec").allowed);
        assert!(!engine.can_use_tool("BashRun").allowed);
        assert!(engine.can_use_tool("Read").allowed);
    }

    #[test]
    fn multiple_deny_read_patterns() {
        let policy = PolicyProfile {
            deny_read: vec![
                "**/.env".to_string(),
                "**/.env.*".to_string(),
                "**/id_rsa".to_string(),
            ],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).expect("compile policy");

        assert!(!engine.can_read_path(Path::new(".env")).allowed);
        assert!(!engine.can_read_path(Path::new("config/.env")).allowed);
        assert!(!engine.can_read_path(Path::new(".env.production")).allowed);
        assert!(!engine.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
        assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn path_traversal_in_read() {
        let policy = PolicyProfile {
            deny_read: vec!["**/etc/passwd".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).expect("compile policy");

        let decision = engine.can_read_path(Path::new("../../etc/passwd"));
        assert!(!decision.allowed);
        assert!(decision.reason.unwrap().contains("denied"));
    }

    #[test]
    fn path_traversal_in_write() {
        let policy = PolicyProfile {
            deny_write: vec!["**/.git/**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).expect("compile policy");

        let decision = engine.can_write_path(Path::new("../.git/config"));
        assert!(!decision.allowed);
        assert!(decision.reason.unwrap().contains("denied"));
    }

    #[test]
    fn complex_policy_combination() {
        let policy = PolicyProfile {
            allowed_tools: vec!["Read".to_string(), "Write".to_string(), "Grep".to_string()],
            disallowed_tools: vec!["Write".to_string()],
            deny_read: vec!["**/.env".to_string()],
            deny_write: vec!["**/locked/**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).expect("compile policy");

        // Write is in allowlist but also in denylist — denylist wins
        assert!(!engine.can_use_tool("Write").allowed);
        // Read is in allowlist and not denied
        assert!(engine.can_use_tool("Read").allowed);
        // Bash is not in allowlist
        assert!(!engine.can_use_tool("Bash").allowed);
        // deny_read blocks .env
        assert!(!engine.can_read_path(Path::new(".env")).allowed);
        assert!(engine.can_read_path(Path::new("src/lib.rs")).allowed);
        // deny_write blocks locked/
        assert!(!engine.can_write_path(Path::new("locked/data.txt")).allowed);
        assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn decision_allow() {
        let d = super::Decision::allow();
        assert!(d.allowed);
        assert!(d.reason.is_none());
    }

    #[test]
    fn decision_deny() {
        let d = super::Decision::deny("not permitted");
        assert!(!d.allowed);
        assert_eq!(d.reason.as_deref(), Some("not permitted"));
    }

    #[test]
    fn wildcard_allowlist_with_specific_deny() {
        let policy = PolicyProfile {
            allowed_tools: vec!["*".to_string()],
            disallowed_tools: vec!["Bash".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).expect("compile policy");

        assert!(!engine.can_use_tool("Bash").allowed);
        assert!(engine.can_use_tool("Read").allowed);
        assert!(engine.can_use_tool("Write").allowed);
    }

    #[test]
    fn deep_nested_path_deny_write() {
        let policy = PolicyProfile {
            deny_write: vec!["secret/**".to_string()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).expect("compile policy");

        assert!(!engine.can_write_path(Path::new("secret/a/b/c.txt")).allowed);
        assert!(!engine.can_write_path(Path::new("secret/x.txt")).allowed);
        assert!(engine.can_write_path(Path::new("public/data.txt")).allowed);
    }

    #[test]
    fn allow_and_deny_network_fields() {
        let policy = PolicyProfile {
            allow_network: vec!["*.example.com".to_string()],
            deny_network: vec!["evil.example.com".to_string()],
            ..PolicyProfile::default()
        };
        // Fields are stored correctly — engine compiles without error
        let _engine = PolicyEngine::new(&policy).expect("compile policy");
        assert_eq!(policy.allow_network, vec!["*.example.com"]);
        assert_eq!(policy.deny_network, vec!["evil.example.com"]);
    }

    #[test]
    fn require_approval_for_field() {
        let policy = PolicyProfile {
            require_approval_for: vec!["Bash".to_string(), "DeleteFile".to_string()],
            ..PolicyProfile::default()
        };
        let _engine = PolicyEngine::new(&policy).expect("compile policy");
        assert_eq!(policy.require_approval_for, vec!["Bash", "DeleteFile"]);
    }
}
