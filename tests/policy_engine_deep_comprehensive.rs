// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive policy engine tests covering construction, compilation,
//! tool/read/write checks, glob patterns, composition, auditing, rules,
//! validation, serialization, and edge cases.

use std::path::Path;

use abp_core::PolicyProfile;
use abp_glob::IncludeExcludeGlobs;
use abp_policy::audit::{AuditSummary, PolicyAuditor, PolicyDecision as AuditDecision};
use abp_policy::compose::{
    ComposedEngine, PolicyDecision, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};
use abp_policy::{Decision, PolicyEngine};

// =========================================================================
// Helpers
// =========================================================================

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn engine(profile: &PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(profile).expect("compile policy")
}

// =========================================================================
// 1. PolicyProfile construction
// =========================================================================

mod profile_construction {
    use super::*;

    #[test]
    fn default_profile_has_empty_fields() {
        let p = PolicyProfile::default();
        assert!(p.allowed_tools.is_empty());
        assert!(p.disallowed_tools.is_empty());
        assert!(p.deny_read.is_empty());
        assert!(p.deny_write.is_empty());
        assert!(p.allow_network.is_empty());
        assert!(p.deny_network.is_empty());
        assert!(p.require_approval_for.is_empty());
    }

    #[test]
    fn profile_with_tools() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read", "Write"]),
            disallowed_tools: s(&["Bash"]),
            ..PolicyProfile::default()
        };
        assert_eq!(p.allowed_tools.len(), 2);
        assert_eq!(p.disallowed_tools.len(), 1);
    }

    #[test]
    fn profile_with_read_paths() {
        let p = PolicyProfile {
            deny_read: s(&["**/.env", "**/secrets/**"]),
            ..PolicyProfile::default()
        };
        assert_eq!(p.deny_read.len(), 2);
    }

    #[test]
    fn profile_with_write_paths() {
        let p = PolicyProfile {
            deny_write: s(&["**/.git/**", "**/node_modules/**"]),
            ..PolicyProfile::default()
        };
        assert_eq!(p.deny_write.len(), 2);
    }

    #[test]
    fn profile_with_all_fields() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read"]),
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["secret.txt"]),
            deny_write: s(&["locked.txt"]),
            allow_network: s(&["*.example.com"]),
            deny_network: s(&["evil.com"]),
            require_approval_for: s(&["DeleteFile"]),
        };
        assert_eq!(p.allowed_tools, s(&["Read"]));
        assert_eq!(p.disallowed_tools, s(&["Bash"]));
        assert_eq!(p.deny_read, s(&["secret.txt"]));
        assert_eq!(p.deny_write, s(&["locked.txt"]));
        assert_eq!(p.allow_network, s(&["*.example.com"]));
        assert_eq!(p.deny_network, s(&["evil.com"]));
        assert_eq!(p.require_approval_for, s(&["DeleteFile"]));
    }

    #[test]
    fn profile_with_network_fields() {
        let p = PolicyProfile {
            allow_network: s(&["api.github.com", "*.npmjs.org"]),
            deny_network: s(&["*.evil.com"]),
            ..PolicyProfile::default()
        };
        assert_eq!(p.allow_network.len(), 2);
        assert_eq!(p.deny_network.len(), 1);
    }

    #[test]
    fn profile_with_approval_requirements() {
        let p = PolicyProfile {
            require_approval_for: s(&["Bash", "DeleteFile", "Write"]),
            ..PolicyProfile::default()
        };
        assert_eq!(p.require_approval_for.len(), 3);
    }
}

// =========================================================================
// 2. PolicyEngine compilation
// =========================================================================

mod compilation {
    use super::*;

    #[test]
    fn empty_profile_compiles() {
        engine(&PolicyProfile::default());
    }

    #[test]
    fn single_allowed_tool_compiles() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read"]),
            ..PolicyProfile::default()
        };
        engine(&p);
    }

    #[test]
    fn multiple_glob_patterns_compile() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read*", "Write*", "Grep*"]),
            disallowed_tools: s(&["Bash*", "Shell*"]),
            deny_read: s(&["**/.env", "**/.env.*", "**/id_rsa"]),
            deny_write: s(&["**/.git/**", "**/node_modules/**"]),
            ..PolicyProfile::default()
        };
        engine(&p);
    }

    #[test]
    fn wildcard_star_compiles() {
        let p = PolicyProfile {
            allowed_tools: s(&["*"]),
            ..PolicyProfile::default()
        };
        engine(&p);
    }

    #[test]
    fn double_star_glob_compiles() {
        let p = PolicyProfile {
            deny_read: s(&["**"]),
            deny_write: s(&["**/*"]),
            ..PolicyProfile::default()
        };
        engine(&p);
    }

    #[test]
    fn complex_brace_patterns_compile() {
        let p = PolicyProfile {
            deny_read: s(&["**/*.{env,key,pem}"]),
            deny_write: s(&["**/*.{lock,bak}"]),
            ..PolicyProfile::default()
        };
        engine(&p);
    }

    #[test]
    fn question_mark_pattern_compiles() {
        let p = PolicyProfile {
            deny_read: s(&["secret?.txt"]),
            ..PolicyProfile::default()
        };
        engine(&p);
    }
}

// =========================================================================
// 3. Tool allow/deny checks
// =========================================================================

mod tool_checks {
    use super::*;

    #[test]
    fn deny_specific_tool() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn deny_tool_reason_message() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..PolicyProfile::default()
        });
        let d = e.can_use_tool("Bash");
        assert_eq!(d.reason.as_deref(), Some("tool 'Bash' is disallowed"));
    }

    #[test]
    fn allow_unlisted_tool_when_no_allowlist() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_use_tool("AnyTool").allowed);
    }

    #[test]
    fn allowlist_blocks_unlisted_tool() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Read", "Write"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("Bash").allowed);
        assert_eq!(
            e.can_use_tool("Bash").reason.as_deref(),
            Some("tool 'Bash' not in allowlist")
        );
    }

    #[test]
    fn allowlist_permits_listed_tool() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Read", "Write"]),
            ..PolicyProfile::default()
        });
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("Write").allowed);
    }

    #[test]
    fn deny_overrides_allow() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["*"]),
            disallowed_tools: s(&["Bash"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(e.can_use_tool("Read").allowed);
    }

    #[test]
    fn glob_pattern_in_deny_tools() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash*"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("BashExec").allowed);
        assert!(!e.can_use_tool("BashRun").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(e.can_use_tool("Read").allowed);
    }

    #[test]
    fn glob_pattern_in_allow_tools() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["File*"]),
            ..PolicyProfile::default()
        });
        assert!(e.can_use_tool("FileRead").allowed);
        assert!(e.can_use_tool("FileWrite").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn multiple_denied_tools() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash", "Shell", "Exec"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_use_tool("Shell").allowed);
        assert!(!e.can_use_tool("Exec").allowed);
        assert!(e.can_use_tool("Read").allowed);
    }

    #[test]
    fn tool_in_both_allow_and_deny() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Write"]),
            disallowed_tools: s(&["Write"]),
            ..PolicyProfile::default()
        });
        // Deny wins
        assert!(!e.can_use_tool("Write").allowed);
    }

    #[test]
    fn case_sensitive_tool_names() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["bash"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("bash").allowed);
        // Glob matching is case-insensitive on Windows, case-sensitive on Unix
        // Just verify the exact match is denied
    }

    #[test]
    fn wildcard_allow_with_multiple_denies() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["*"]),
            disallowed_tools: s(&["Bash", "Shell", "Exec"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_use_tool("Shell").allowed);
        assert!(!e.can_use_tool("Exec").allowed);
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("Write").allowed);
        assert!(e.can_use_tool("Grep").allowed);
    }
}

// =========================================================================
// 4. Read path allow/deny checks
// =========================================================================

mod read_path_checks {
    use super::*;

    #[test]
    fn deny_specific_file() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["secret.txt"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
    }

    #[test]
    fn allow_non_denied_file() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["secret.txt"]),
            ..PolicyProfile::default()
        });
        assert!(e.can_read_path(Path::new("public.txt")).allowed);
    }

    #[test]
    fn deny_read_with_glob_star() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["secret*"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("secret.txt")).allowed);
        assert!(!e.can_read_path(Path::new("secrets.json")).allowed);
        assert!(e.can_read_path(Path::new("public.txt")).allowed);
    }

    #[test]
    fn deny_read_with_double_star() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/.env"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_read_path(Path::new("config/.env")).allowed);
        assert!(!e.can_read_path(Path::new("a/b/c/.env")).allowed);
        assert!(e.can_read_path(Path::new(".env.local")).allowed);
    }

    #[test]
    fn deny_read_deep_nested() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/secrets/**"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("secrets/key.pem")).allowed);
        assert!(!e.can_read_path(Path::new("a/secrets/key.pem")).allowed);
        assert!(!e.can_read_path(Path::new("a/b/secrets/c/d/e.txt")).allowed);
        assert!(e.can_read_path(Path::new("public/data.txt")).allowed);
    }

    #[test]
    fn deny_read_multiple_patterns() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/.env", "**/.env.*", "**/id_rsa", "**/*.key"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_read_path(Path::new(".env.production")).allowed);
        assert!(!e.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
        assert!(!e.can_read_path(Path::new("certs/server.key")).allowed);
        assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn deny_read_with_extension_glob() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/*.pem", "**/*.key", "**/*.p12"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
        assert!(!e.can_read_path(Path::new("deep/path/to/file.key")).allowed);
        assert!(!e.can_read_path(Path::new("store.p12")).allowed);
        assert!(e.can_read_path(Path::new("file.txt")).allowed);
    }

    #[test]
    fn deny_read_reason_message() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["secret.txt"]),
            ..PolicyProfile::default()
        });
        let d = e.can_read_path(Path::new("secret.txt"));
        assert!(!d.allowed);
        assert!(d.reason.unwrap().contains("denied"));
    }

    #[test]
    fn path_traversal_in_read() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/etc/passwd"]),
            ..PolicyProfile::default()
        });
        let d = e.can_read_path(Path::new("../../etc/passwd"));
        assert!(!d.allowed);
    }

    #[test]
    fn deny_read_brace_expansion() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/*.{env,key,pem}"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("data.env")).allowed);
        assert!(!e.can_read_path(Path::new("cert.key")).allowed);
        assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
        assert!(e.can_read_path(Path::new("file.txt")).allowed);
    }
}

// =========================================================================
// 5. Write path allow/deny checks
// =========================================================================

mod write_path_checks {
    use super::*;

    #[test]
    fn deny_specific_write_file() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["locked.txt"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_write_path(Path::new("locked.txt")).allowed);
    }

    #[test]
    fn allow_non_denied_write() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["locked.txt"]),
            ..PolicyProfile::default()
        });
        assert!(e.can_write_path(Path::new("writable.txt")).allowed);
    }

    #[test]
    fn deny_write_git_directory() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/.git/**"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_write_path(Path::new(".git/config")).allowed);
        assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
        assert!(!e.can_write_path(Path::new(".git/objects/ab/cd")).allowed);
        assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn deny_write_node_modules() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/node_modules/**"]),
            ..PolicyProfile::default()
        });
        assert!(
            !e.can_write_path(Path::new("node_modules/foo/index.js"))
                .allowed
        );
        assert!(
            !e.can_write_path(Path::new("pkg/node_modules/bar.js"))
                .allowed
        );
        assert!(e.can_write_path(Path::new("src/index.js")).allowed);
    }

    #[test]
    fn deny_write_deep_nested() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["secret/**"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_write_path(Path::new("secret/a/b/c.txt")).allowed);
        assert!(!e.can_write_path(Path::new("secret/x.txt")).allowed);
        assert!(e.can_write_path(Path::new("public/data.txt")).allowed);
    }

    #[test]
    fn deny_write_multiple_patterns() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/.git/**", "**/node_modules/**", "**/*.lock"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_write_path(Path::new(".git/config")).allowed);
        assert!(!e.can_write_path(Path::new("node_modules/x.js")).allowed);
        assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
        assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn deny_write_reason_message() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["locked.txt"]),
            ..PolicyProfile::default()
        });
        let d = e.can_write_path(Path::new("locked.txt"));
        assert!(!d.allowed);
        assert!(d.reason.unwrap().contains("denied"));
    }

    #[test]
    fn path_traversal_in_write() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/.git/**"]),
            ..PolicyProfile::default()
        });
        let d = e.can_write_path(Path::new("../.git/config"));
        assert!(!d.allowed);
    }

    #[test]
    fn deny_write_with_extension_glob() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/*.{lock,bak,tmp}"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
        assert!(!e.can_write_path(Path::new("data.bak")).allowed);
        assert!(!e.can_write_path(Path::new("file.tmp")).allowed);
        assert!(e.can_write_path(Path::new("file.txt")).allowed);
    }
}

// =========================================================================
// 6. Default-deny behavior
// =========================================================================

mod default_deny {
    use super::*;

    #[test]
    fn empty_policy_allows_all_tools() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_use_tool("Bash").allowed);
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("Write").allowed);
        assert!(e.can_use_tool("AnyTool").allowed);
    }

    #[test]
    fn empty_policy_allows_all_reads() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_read_path(Path::new("any/file.txt")).allowed);
        assert!(e.can_read_path(Path::new(".env")).allowed);
        assert!(e.can_read_path(Path::new("secret.key")).allowed);
    }

    #[test]
    fn empty_policy_allows_all_writes() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_write_path(Path::new("any/file.txt")).allowed);
        assert!(e.can_write_path(Path::new(".git/config")).allowed);
    }

    #[test]
    fn allowlist_creates_default_deny_for_tools() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Read"]),
            ..PolicyProfile::default()
        });
        // Only Read is allowed; everything else is denied
        assert!(e.can_use_tool("Read").allowed);
        assert!(!e.can_use_tool("Write").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn deny_all_reads_with_catch_all() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("any.txt")).allowed);
        assert!(!e.can_read_path(Path::new("deep/path/file.rs")).allowed);
    }

    #[test]
    fn deny_all_writes_with_catch_all() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_write_path(Path::new("any.txt")).allowed);
        assert!(!e.can_write_path(Path::new("deep/path/file.rs")).allowed);
    }

    #[test]
    fn deny_all_tools_with_wildcard() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["*"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_use_tool("Read").allowed);
        assert!(!e.can_use_tool("Write").allowed);
    }
}

// =========================================================================
// 7. Wildcard patterns
// =========================================================================

mod wildcard_patterns {
    use super::*;

    #[test]
    fn single_star_matches_txt_extension() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["*.txt"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("file.txt")).allowed);
        assert!(!e.can_read_path(Path::new("other.txt")).allowed);
        // globset * also matches across directories in path context
        assert!(!e.can_read_path(Path::new("dir/file.txt")).allowed);
        assert!(e.can_read_path(Path::new("file.rs")).allowed);
    }

    #[test]
    fn double_star_crosses_directories() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/*.txt"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("file.txt")).allowed);
        assert!(!e.can_read_path(Path::new("dir/file.txt")).allowed);
        assert!(!e.can_read_path(Path::new("a/b/c/file.txt")).allowed);
    }

    #[test]
    fn star_in_tool_name_matches_suffix() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["File*"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("FileRead").allowed);
        assert!(!e.can_use_tool("FileWrite").allowed);
        assert!(!e.can_use_tool("FileDelete").allowed);
        assert!(e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn double_star_slash_pattern() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/build/**"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_write_path(Path::new("build/output.bin")).allowed);
        assert!(
            !e.can_write_path(Path::new("project/build/output.bin"))
                .allowed
        );
        assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn question_mark_matches_single_char() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["secret?.txt"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("secret1.txt")).allowed);
        assert!(!e.can_read_path(Path::new("secretA.txt")).allowed);
        assert!(e.can_read_path(Path::new("secret.txt")).allowed);
        assert!(e.can_read_path(Path::new("secret12.txt")).allowed);
    }

    #[test]
    fn wildcard_star_in_allowed_tools() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["*"]),
            ..PolicyProfile::default()
        });
        assert!(e.can_use_tool("Bash").allowed);
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("AnyTool").allowed);
    }
}

// =========================================================================
// 8. Complex glob patterns
// =========================================================================

mod complex_globs {
    use super::*;

    #[test]
    fn brace_expansion_multiple_extensions() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/*.{pem,key,p12,pfx}"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
        assert!(!e.can_read_path(Path::new("private.key")).allowed);
        assert!(!e.can_read_path(Path::new("store.p12")).allowed);
        assert!(!e.can_read_path(Path::new("store.pfx")).allowed);
        assert!(e.can_read_path(Path::new("cert.crt")).allowed);
    }

    #[test]
    fn multiple_directory_patterns() {
        let e = engine(&PolicyProfile {
            deny_write: s(&[
                "**/.git/**",
                "**/node_modules/**",
                "**/target/**",
                "**/.cache/**",
            ]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
        assert!(!e.can_write_path(Path::new("node_modules/x/y.js")).allowed);
        assert!(!e.can_write_path(Path::new("target/debug/main")).allowed);
        assert!(!e.can_write_path(Path::new(".cache/data")).allowed);
        assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn nested_directory_with_extension() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["config/**/*.secret"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("config/db.secret")).allowed);
        assert!(
            !e.can_read_path(Path::new("config/nested/api.secret"))
                .allowed
        );
        assert!(e.can_read_path(Path::new("config/db.json")).allowed);
        assert!(e.can_read_path(Path::new("other/file.secret")).allowed);
    }

    #[test]
    fn pattern_with_leading_dot_directory() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/.ssh/**"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new(".ssh/id_rsa")).allowed);
        assert!(!e.can_read_path(Path::new("home/.ssh/config")).allowed);
        assert!(e.can_read_path(Path::new("ssh/config")).allowed);
    }

    #[test]
    fn pattern_matching_exact_filename_any_depth() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/Makefile"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("Makefile")).allowed);
        assert!(!e.can_read_path(Path::new("sub/Makefile")).allowed);
        assert!(!e.can_read_path(Path::new("a/b/c/Makefile")).allowed);
        assert!(e.can_read_path(Path::new("Makefile.bak")).allowed);
    }
}

// =========================================================================
// 9. Include and exclude patterns together (via IncludeExcludeGlobs)
// =========================================================================

mod include_exclude {
    use super::*;

    #[test]
    fn include_only() {
        let g = IncludeExcludeGlobs::new(&s(&["src/**"]), &[]).unwrap();
        assert!(g.decide_str("src/main.rs").is_allowed());
        assert!(!g.decide_str("tests/test.rs").is_allowed());
    }

    #[test]
    fn exclude_only() {
        let g = IncludeExcludeGlobs::new(&[], &s(&["*.log"])).unwrap();
        assert!(g.decide_str("file.txt").is_allowed());
        assert!(!g.decide_str("error.log").is_allowed());
    }

    #[test]
    fn include_and_exclude() {
        let g = IncludeExcludeGlobs::new(&s(&["src/**"]), &s(&["src/generated/**"])).unwrap();
        assert!(g.decide_str("src/main.rs").is_allowed());
        assert!(!g.decide_str("src/generated/output.rs").is_allowed());
        assert!(!g.decide_str("tests/test.rs").is_allowed());
    }

    #[test]
    fn empty_include_and_exclude_allows_all() {
        let g = IncludeExcludeGlobs::new(&[] as &[String], &[] as &[String]).unwrap();
        assert!(g.decide_str("anything.txt").is_allowed());
        assert!(g.decide_str("deep/path/to/file.rs").is_allowed());
    }

    #[test]
    fn exclude_overrides_include() {
        let g = IncludeExcludeGlobs::new(&s(&["**/*.rs"]), &s(&["**/test_*.rs"])).unwrap();
        assert!(g.decide_str("src/main.rs").is_allowed());
        assert!(!g.decide_str("tests/test_policy.rs").is_allowed());
    }

    #[test]
    fn decide_path_vs_decide_str() {
        let g = IncludeExcludeGlobs::new(&[], &s(&["**/.env"])).unwrap();
        assert!(!g.decide_path(Path::new(".env")).is_allowed());
        assert!(!g.decide_str(".env").is_allowed());
        assert!(g.decide_path(Path::new("src/main.rs")).is_allowed());
        assert!(g.decide_str("src/main.rs").is_allowed());
    }

    #[test]
    fn tool_allowlist_creates_include_with_exclude() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Read*", "Write*"]),
            disallowed_tools: s(&["WriteSecret"]),
            ..PolicyProfile::default()
        });
        assert!(e.can_use_tool("ReadFile").allowed);
        assert!(e.can_use_tool("WriteFile").allowed);
        assert!(!e.can_use_tool("WriteSecret").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
    }
}

// =========================================================================
// 10. Empty policy (allow all vs deny all)
// =========================================================================

mod empty_policy {
    use super::*;

    #[test]
    fn completely_empty_allows_everything() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_use_tool("Anything").allowed);
        assert!(e.can_read_path(Path::new("any/path")).allowed);
        assert!(e.can_write_path(Path::new("any/path")).allowed);
    }

    #[test]
    fn empty_allowed_tools_means_backend_default() {
        let e = engine(&PolicyProfile {
            allowed_tools: vec![],
            ..PolicyProfile::default()
        });
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn empty_deny_read_allows_all_reads() {
        let e = engine(&PolicyProfile {
            deny_read: vec![],
            ..PolicyProfile::default()
        });
        assert!(e.can_read_path(Path::new("any/path")).allowed);
    }

    #[test]
    fn empty_deny_write_allows_all_writes() {
        let e = engine(&PolicyProfile {
            deny_write: vec![],
            ..PolicyProfile::default()
        });
        assert!(e.can_write_path(Path::new("any/path")).allowed);
    }
}

// =========================================================================
// 11. Serialization / deserialization round-trip
// =========================================================================

mod serde_roundtrip {
    use super::*;

    #[test]
    fn policy_profile_json_roundtrip() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read", "Write"]),
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["**/.env"]),
            deny_write: s(&["**/.git/**"]),
            allow_network: s(&["*.example.com"]),
            deny_network: s(&["evil.com"]),
            require_approval_for: s(&["DeleteFile"]),
        };
        let json = serde_json::to_string(&p).unwrap();
        let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(p.allowed_tools, p2.allowed_tools);
        assert_eq!(p.disallowed_tools, p2.disallowed_tools);
        assert_eq!(p.deny_read, p2.deny_read);
        assert_eq!(p.deny_write, p2.deny_write);
        assert_eq!(p.allow_network, p2.allow_network);
        assert_eq!(p.deny_network, p2.deny_network);
        assert_eq!(p.require_approval_for, p2.require_approval_for);
    }

    #[test]
    fn empty_profile_json_roundtrip() {
        let p = PolicyProfile::default();
        let json = serde_json::to_string(&p).unwrap();
        let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
        assert!(p2.allowed_tools.is_empty());
        assert!(p2.disallowed_tools.is_empty());
        assert!(p2.deny_read.is_empty());
        assert!(p2.deny_write.is_empty());
    }

    #[test]
    fn decision_serde_roundtrip() {
        let d = Decision::deny("not allowed");
        let json = serde_json::to_string(&d).unwrap();
        let d2: Decision = serde_json::from_str(&json).unwrap();
        assert!(!d2.allowed);
        assert_eq!(d2.reason.as_deref(), Some("not allowed"));
    }

    #[test]
    fn decision_allow_serde_roundtrip() {
        let d = Decision::allow();
        let json = serde_json::to_string(&d).unwrap();
        let d2: Decision = serde_json::from_str(&json).unwrap();
        assert!(d2.allowed);
        assert!(d2.reason.is_none());
    }

    #[test]
    fn policy_profile_preserves_order() {
        let p = PolicyProfile {
            allowed_tools: s(&["Z", "A", "M"]),
            ..PolicyProfile::default()
        };
        let json = serde_json::to_string(&p).unwrap();
        let p2: PolicyProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(p2.allowed_tools, s(&["Z", "A", "M"]));
    }

    #[test]
    fn policy_decision_compose_serde() {
        let d = PolicyDecision::Deny {
            reason: "not allowed".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let d2: PolicyDecision = serde_json::from_str(&json).unwrap();
        assert!(d2.is_deny());
    }

    #[test]
    fn policy_decision_allow_serde() {
        let d = PolicyDecision::Allow {
            reason: "permitted".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let d2: PolicyDecision = serde_json::from_str(&json).unwrap();
        assert!(d2.is_allow());
    }

    #[test]
    fn policy_decision_abstain_serde() {
        let d = PolicyDecision::Abstain;
        let json = serde_json::to_string(&d).unwrap();
        let d2: PolicyDecision = serde_json::from_str(&json).unwrap();
        assert!(d2.is_abstain());
    }

    #[test]
    fn audit_decision_serde() {
        let d = AuditDecision::Deny {
            reason: "blocked".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let d2: AuditDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(d, d2);
    }

    #[test]
    fn rule_effect_serde() {
        let eff = RuleEffect::Throttle { max: 100 };
        let json = serde_json::to_string(&eff).unwrap();
        let eff2: RuleEffect = serde_json::from_str(&json).unwrap();
        assert_eq!(eff, eff2);
    }

    #[test]
    fn rule_condition_serde() {
        let cond = RuleCondition::And(vec![
            RuleCondition::Pattern("Bash*".into()),
            RuleCondition::Not(Box::new(RuleCondition::Never)),
        ]);
        let json = serde_json::to_string(&cond).unwrap();
        let cond2: RuleCondition = serde_json::from_str(&json).unwrap();
        assert!(cond2.matches("BashExec"));
    }
}

// =========================================================================
// 12. PolicySet merge (multiple profiles merged)
// =========================================================================

mod policy_set_merge {
    use super::*;

    #[test]
    fn merge_two_profiles() {
        let mut set = PolicySet::new("test");
        set.add(PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["secret.txt"]),
            ..PolicyProfile::default()
        });
        set.add(PolicyProfile {
            disallowed_tools: s(&["Shell"]),
            deny_write: s(&["locked.txt"]),
            ..PolicyProfile::default()
        });
        let merged = set.merge();
        assert!(merged.disallowed_tools.contains(&"Bash".to_string()));
        assert!(merged.disallowed_tools.contains(&"Shell".to_string()));
        assert!(merged.deny_read.contains(&"secret.txt".to_string()));
        assert!(merged.deny_write.contains(&"locked.txt".to_string()));
    }

    #[test]
    fn merge_deduplicates() {
        let mut set = PolicySet::new("dedup");
        set.add(PolicyProfile {
            disallowed_tools: s(&["Bash", "Shell"]),
            ..PolicyProfile::default()
        });
        set.add(PolicyProfile {
            disallowed_tools: s(&["Bash", "Exec"]),
            ..PolicyProfile::default()
        });
        let merged = set.merge();
        assert_eq!(
            merged
                .disallowed_tools
                .iter()
                .filter(|t| *t == "Bash")
                .count(),
            1
        );
    }

    #[test]
    fn merge_empty_set() {
        let set = PolicySet::new("empty");
        let merged = set.merge();
        assert!(merged.allowed_tools.is_empty());
        assert!(merged.disallowed_tools.is_empty());
    }

    #[test]
    fn merge_three_profiles() {
        let mut set = PolicySet::new("three");
        set.add(PolicyProfile {
            allowed_tools: s(&["Read"]),
            ..PolicyProfile::default()
        });
        set.add(PolicyProfile {
            allowed_tools: s(&["Write"]),
            ..PolicyProfile::default()
        });
        set.add(PolicyProfile {
            allowed_tools: s(&["Grep"]),
            ..PolicyProfile::default()
        });
        let merged = set.merge();
        assert_eq!(merged.allowed_tools.len(), 3);
    }

    #[test]
    fn merged_profile_compiles_to_engine() {
        let mut set = PolicySet::new("compile");
        set.add(PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["**/.env"]),
            ..PolicyProfile::default()
        });
        set.add(PolicyProfile {
            deny_write: s(&["**/.git/**"]),
            ..PolicyProfile::default()
        });
        let merged = set.merge();
        let e = engine(&merged);
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    }

    #[test]
    fn policy_set_name() {
        let set = PolicySet::new("my_set");
        assert_eq!(set.name(), "my_set");
    }

    #[test]
    fn merge_network_and_approval_fields() {
        let mut set = PolicySet::new("net");
        set.add(PolicyProfile {
            allow_network: s(&["api.example.com"]),
            require_approval_for: s(&["Bash"]),
            ..PolicyProfile::default()
        });
        set.add(PolicyProfile {
            deny_network: s(&["evil.com"]),
            require_approval_for: s(&["Delete"]),
            ..PolicyProfile::default()
        });
        let merged = set.merge();
        assert!(
            merged
                .allow_network
                .contains(&"api.example.com".to_string())
        );
        assert!(merged.deny_network.contains(&"evil.com".to_string()));
        assert_eq!(merged.require_approval_for.len(), 2);
    }
}

// =========================================================================
// 13. ComposedEngine precedence strategies
// =========================================================================

mod composed_engine {
    use super::*;

    #[test]
    fn deny_overrides_denies_when_any_deny() {
        let policies = vec![
            PolicyProfile::default(),
            PolicyProfile {
                disallowed_tools: s(&["Bash"]),
                ..PolicyProfile::default()
            },
        ];
        let ce = ComposedEngine::new(policies, PolicyPrecedence::DenyOverrides).unwrap();
        assert!(ce.check_tool("Bash").is_deny());
    }

    #[test]
    fn allow_overrides_allows_when_any_allow() {
        let policies = vec![
            PolicyProfile::default(),
            PolicyProfile {
                disallowed_tools: s(&["Bash"]),
                ..PolicyProfile::default()
            },
        ];
        let ce = ComposedEngine::new(policies, PolicyPrecedence::AllowOverrides).unwrap();
        // First profile allows Bash (no restrictions), so allow wins
        assert!(ce.check_tool("Bash").is_allow());
    }

    #[test]
    fn first_applicable_uses_first_non_abstain() {
        let policies = vec![
            PolicyProfile {
                disallowed_tools: s(&["Bash"]),
                ..PolicyProfile::default()
            },
            PolicyProfile::default(),
        ];
        let ce = ComposedEngine::new(policies, PolicyPrecedence::FirstApplicable).unwrap();
        assert!(ce.check_tool("Bash").is_deny());
    }

    #[test]
    fn empty_composed_engine_abstains() {
        let ce = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides).unwrap();
        assert!(ce.check_tool("Bash").is_abstain());
        assert!(ce.check_read("file.txt").is_abstain());
        assert!(ce.check_write("file.txt").is_abstain());
    }

    #[test]
    fn composed_engine_check_read() {
        let policies = vec![PolicyProfile {
            deny_read: s(&["**/.env"]),
            ..PolicyProfile::default()
        }];
        let ce = ComposedEngine::new(policies, PolicyPrecedence::DenyOverrides).unwrap();
        assert!(ce.check_read(".env").is_deny());
        assert!(ce.check_read("src/main.rs").is_allow());
    }

    #[test]
    fn composed_engine_check_write() {
        let policies = vec![PolicyProfile {
            deny_write: s(&["**/.git/**"]),
            ..PolicyProfile::default()
        }];
        let ce = ComposedEngine::new(policies, PolicyPrecedence::DenyOverrides).unwrap();
        assert!(ce.check_write(".git/config").is_deny());
        assert!(ce.check_write("src/main.rs").is_allow());
    }

    #[test]
    fn deny_overrides_with_multiple_profiles() {
        let policies = vec![
            PolicyProfile {
                deny_read: s(&["**/.env"]),
                ..PolicyProfile::default()
            },
            PolicyProfile {
                deny_write: s(&["**/.git/**"]),
                ..PolicyProfile::default()
            },
        ];
        let ce = ComposedEngine::new(policies, PolicyPrecedence::DenyOverrides).unwrap();
        assert!(ce.check_read(".env").is_deny());
        assert!(ce.check_write(".git/config").is_deny());
        assert!(ce.check_read("src/main.rs").is_allow());
    }
}

// =========================================================================
// 14. PolicyValidator
// =========================================================================

mod validation {
    use super::*;

    #[test]
    fn empty_profile_has_no_warnings() {
        let warnings = PolicyValidator::validate(&PolicyProfile::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn detects_empty_glob_in_allowed_tools() {
        let p = PolicyProfile {
            allowed_tools: vec!["".to_string()],
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn detects_empty_glob_in_disallowed_tools() {
        let p = PolicyProfile {
            disallowed_tools: vec!["".to_string()],
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn detects_empty_glob_in_deny_read() {
        let p = PolicyProfile {
            deny_read: vec!["".to_string()],
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn detects_empty_glob_in_deny_write() {
        let p = PolicyProfile {
            deny_write: vec!["".to_string()],
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));
    }

    #[test]
    fn detects_overlapping_allow_deny_tools() {
        let p = PolicyProfile {
            allowed_tools: s(&["Bash"]),
            disallowed_tools: s(&["Bash"]),
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::OverlappingAllowDeny)
        );
    }

    #[test]
    fn detects_overlapping_allow_deny_network() {
        let p = PolicyProfile {
            allow_network: s(&["evil.com"]),
            deny_network: s(&["evil.com"]),
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::OverlappingAllowDeny)
        );
    }

    #[test]
    fn detects_unreachable_rules_with_wildcard_deny() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read"]),
            disallowed_tools: s(&["*"]),
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::UnreachableRule)
        );
    }

    #[test]
    fn detects_catch_all_deny_read() {
        let p = PolicyProfile {
            deny_read: s(&["**"]),
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::UnreachableRule)
        );
    }

    #[test]
    fn detects_catch_all_deny_write() {
        let p = PolicyProfile {
            deny_write: s(&["**/*"]),
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::UnreachableRule)
        );
    }

    #[test]
    fn no_overlap_with_different_patterns() {
        let p = PolicyProfile {
            allowed_tools: s(&["Read"]),
            disallowed_tools: s(&["Bash"]),
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(
            !warnings
                .iter()
                .any(|w| w.kind == WarningKind::OverlappingAllowDeny)
        );
    }

    #[test]
    fn multiple_empty_globs_produce_multiple_warnings() {
        let p = PolicyProfile {
            allowed_tools: vec!["".to_string(), "".to_string()],
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        let empty_count = warnings
            .iter()
            .filter(|w| w.kind == WarningKind::EmptyGlob)
            .count();
        assert_eq!(empty_count, 2);
    }
}

// =========================================================================
// 15. Auditing
// =========================================================================

mod auditing {
    use super::*;

    #[test]
    fn auditor_records_tool_check() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..PolicyProfile::default()
        });
        let mut auditor = PolicyAuditor::new(e);
        let d = auditor.check_tool("Bash");
        assert!(matches!(d, AuditDecision::Deny { .. }));
        assert_eq!(auditor.entries().len(), 1);
    }

    #[test]
    fn auditor_records_read_check() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["secret.txt"]),
            ..PolicyProfile::default()
        });
        let mut auditor = PolicyAuditor::new(e);
        auditor.check_read("secret.txt");
        assert_eq!(auditor.entries().len(), 1);
        assert_eq!(auditor.denied_count(), 1);
    }

    #[test]
    fn auditor_records_write_check() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["locked.txt"]),
            ..PolicyProfile::default()
        });
        let mut auditor = PolicyAuditor::new(e);
        auditor.check_write("locked.txt");
        assert_eq!(auditor.entries().len(), 1);
        assert_eq!(auditor.denied_count(), 1);
    }

    #[test]
    fn auditor_counts_allowed_and_denied() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..PolicyProfile::default()
        });
        let mut auditor = PolicyAuditor::new(e);
        auditor.check_tool("Bash");
        auditor.check_tool("Read");
        auditor.check_tool("Write");
        assert_eq!(auditor.denied_count(), 1);
        assert_eq!(auditor.allowed_count(), 2);
    }

    #[test]
    fn auditor_summary() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["secret.txt"]),
            ..PolicyProfile::default()
        });
        let mut auditor = PolicyAuditor::new(e);
        auditor.check_tool("Bash");
        auditor.check_tool("Read");
        auditor.check_read("secret.txt");
        auditor.check_read("public.txt");
        let summary = auditor.summary();
        assert_eq!(
            summary,
            AuditSummary {
                allowed: 2,
                denied: 2,
                warned: 0,
            }
        );
    }

    #[test]
    fn auditor_entries_preserve_order() {
        let e = engine(&PolicyProfile::default());
        let mut auditor = PolicyAuditor::new(e);
        auditor.check_tool("First");
        auditor.check_tool("Second");
        auditor.check_tool("Third");
        let entries = auditor.entries();
        assert_eq!(entries[0].resource, "First");
        assert_eq!(entries[1].resource, "Second");
        assert_eq!(entries[2].resource, "Third");
    }

    #[test]
    fn auditor_entry_has_action_field() {
        let e = engine(&PolicyProfile::default());
        let mut auditor = PolicyAuditor::new(e);
        auditor.check_tool("Read");
        auditor.check_read("file.txt");
        auditor.check_write("file.txt");
        assert_eq!(auditor.entries()[0].action, "tool");
        assert_eq!(auditor.entries()[1].action, "read");
        assert_eq!(auditor.entries()[2].action, "write");
    }
}

// =========================================================================
// 16. RuleEngine
// =========================================================================

mod rule_engine {
    use super::*;

    #[test]
    fn empty_rule_engine_allows() {
        let re = RuleEngine::new();
        assert_eq!(re.evaluate("anything"), RuleEffect::Allow);
    }

    #[test]
    fn single_deny_rule() {
        let mut re = RuleEngine::new();
        re.add_rule(Rule {
            id: "r1".into(),
            description: "Deny bash".into(),
            condition: RuleCondition::Pattern("Bash*".into()),
            effect: RuleEffect::Deny,
            priority: 1,
        });
        assert_eq!(re.evaluate("BashExec"), RuleEffect::Deny);
        assert_eq!(re.evaluate("Read"), RuleEffect::Allow);
    }

    #[test]
    fn higher_priority_wins() {
        let mut re = RuleEngine::new();
        re.add_rule(Rule {
            id: "r1".into(),
            description: "Deny all".into(),
            condition: RuleCondition::Always,
            effect: RuleEffect::Deny,
            priority: 1,
        });
        re.add_rule(Rule {
            id: "r2".into(),
            description: "Allow all".into(),
            condition: RuleCondition::Always,
            effect: RuleEffect::Allow,
            priority: 10,
        });
        assert_eq!(re.evaluate("anything"), RuleEffect::Allow);
    }

    #[test]
    fn rule_condition_always() {
        assert!(RuleCondition::Always.matches("anything"));
    }

    #[test]
    fn rule_condition_never() {
        assert!(!RuleCondition::Never.matches("anything"));
    }

    #[test]
    fn rule_condition_pattern() {
        let cond = RuleCondition::Pattern("Bash*".into());
        assert!(cond.matches("BashExec"));
        assert!(!cond.matches("Read"));
    }

    #[test]
    fn rule_condition_and() {
        let cond = RuleCondition::And(vec![
            RuleCondition::Always,
            RuleCondition::Pattern("B*".into()),
        ]);
        assert!(cond.matches("Bash"));
        assert!(!cond.matches("Read"));
    }

    #[test]
    fn rule_condition_or() {
        let cond = RuleCondition::Or(vec![
            RuleCondition::Pattern("Bash*".into()),
            RuleCondition::Pattern("Shell*".into()),
        ]);
        assert!(cond.matches("BashExec"));
        assert!(cond.matches("ShellRun"));
        assert!(!cond.matches("Read"));
    }

    #[test]
    fn rule_condition_not() {
        let cond = RuleCondition::Not(Box::new(RuleCondition::Pattern("Bash*".into())));
        assert!(!cond.matches("BashExec"));
        assert!(cond.matches("Read"));
    }

    #[test]
    fn evaluate_all_returns_all_rules() {
        let mut re = RuleEngine::new();
        re.add_rule(Rule {
            id: "r1".into(),
            description: "".into(),
            condition: RuleCondition::Always,
            effect: RuleEffect::Allow,
            priority: 1,
        });
        re.add_rule(Rule {
            id: "r2".into(),
            description: "".into(),
            condition: RuleCondition::Never,
            effect: RuleEffect::Deny,
            priority: 2,
        });
        let results = re.evaluate_all("test");
        assert_eq!(results.len(), 2);
        assert!(results[0].matched);
        assert!(!results[1].matched);
    }

    #[test]
    fn remove_rule() {
        let mut re = RuleEngine::new();
        re.add_rule(Rule {
            id: "r1".into(),
            description: "".into(),
            condition: RuleCondition::Always,
            effect: RuleEffect::Deny,
            priority: 1,
        });
        assert_eq!(re.rule_count(), 1);
        re.remove_rule("r1");
        assert_eq!(re.rule_count(), 0);
    }

    #[test]
    fn remove_nonexistent_rule_is_noop() {
        let mut re = RuleEngine::new();
        re.remove_rule("nope");
        assert_eq!(re.rule_count(), 0);
    }

    #[test]
    fn throttle_effect() {
        let mut re = RuleEngine::new();
        re.add_rule(Rule {
            id: "t1".into(),
            description: "Throttle bash".into(),
            condition: RuleCondition::Pattern("Bash*".into()),
            effect: RuleEffect::Throttle { max: 5 },
            priority: 1,
        });
        assert_eq!(re.evaluate("BashExec"), RuleEffect::Throttle { max: 5 });
    }

    #[test]
    fn log_effect() {
        let mut re = RuleEngine::new();
        re.add_rule(Rule {
            id: "l1".into(),
            description: "Log reads".into(),
            condition: RuleCondition::Pattern("Read*".into()),
            effect: RuleEffect::Log,
            priority: 1,
        });
        assert_eq!(re.evaluate("ReadFile"), RuleEffect::Log);
    }

    #[test]
    fn nested_conditions_and_or_not() {
        let cond = RuleCondition::And(vec![
            RuleCondition::Or(vec![
                RuleCondition::Pattern("File*".into()),
                RuleCondition::Pattern("Dir*".into()),
            ]),
            RuleCondition::Not(Box::new(RuleCondition::Pattern("FileDelete".into()))),
        ]);
        assert!(cond.matches("FileRead"));
        assert!(cond.matches("DirList"));
        assert!(!cond.matches("FileDelete"));
        assert!(!cond.matches("Bash"));
    }
}

// =========================================================================
// 17. Path normalization
// =========================================================================

mod path_normalization {
    use super::*;

    #[test]
    fn forward_slash_paths() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["secrets/key.pem"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("secrets/key.pem")).allowed);
    }

    #[test]
    fn relative_path_with_dot() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/.env"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("./.env")).allowed);
    }

    #[test]
    fn path_with_double_dots() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["**/.git/**"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_write_path(Path::new("../.git/config")).allowed);
    }

    #[test]
    fn deeply_nested_path() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/*.secret"]),
            ..PolicyProfile::default()
        });
        assert!(
            !e.can_read_path(Path::new("a/b/c/d/e/f/g/data.secret"))
                .allowed
        );
        assert!(e.can_read_path(Path::new("a/b/c/d/e/f/g/data.txt")).allowed);
    }

    #[test]
    fn root_relative_path() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/passwd"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("etc/passwd")).allowed);
    }
}

// =========================================================================
// 18. Edge cases
// =========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn empty_tool_name() {
        let e = engine(&PolicyProfile::default());
        assert!(e.can_use_tool("").allowed);
    }

    #[test]
    fn empty_tool_name_with_allowlist() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Read"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("").allowed);
    }

    #[test]
    fn tool_name_with_spaces() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["My Tool"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("My Tool").allowed);
        assert!(e.can_use_tool("MyTool").allowed);
    }

    #[test]
    fn tool_name_with_special_chars() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["tool-v2"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("tool-v2").allowed);
    }

    #[test]
    fn very_long_path() {
        let long_path = "a/".repeat(100) + "file.txt";
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/*.txt"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new(&long_path)).allowed);
    }

    #[test]
    fn path_with_spaces() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/*.txt"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("my dir/my file.txt")).allowed);
    }

    #[test]
    fn path_with_unicode() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/*.txt"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("données/café.txt")).allowed);
    }

    #[test]
    fn single_char_tool_name() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["X"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("X").allowed);
        assert!(e.can_use_tool("Y").allowed);
    }

    #[test]
    fn tool_name_with_dots() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["file.read"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("file.read").allowed);
    }

    #[test]
    fn tool_name_with_underscores() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["bash_exec"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("bash_exec").allowed);
    }

    #[test]
    fn numeric_tool_name() {
        let e = engine(&PolicyProfile {
            disallowed_tools: s(&["123"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_use_tool("123").allowed);
    }

    #[test]
    fn many_deny_patterns() {
        let patterns: Vec<String> = (0..50).map(|i| format!("**/deny_{i}/**")).collect();
        let p = PolicyProfile {
            deny_read: patterns,
            ..PolicyProfile::default()
        };
        let e = engine(&p);
        assert!(!e.can_read_path(Path::new("deny_0/file.txt")).allowed);
        assert!(!e.can_read_path(Path::new("deny_49/file.txt")).allowed);
        assert!(e.can_read_path(Path::new("allowed/file.txt")).allowed);
    }

    #[test]
    fn decision_debug_format() {
        let d = Decision::deny("reason");
        let dbg = format!("{d:?}");
        assert!(dbg.contains("allowed"));
        assert!(dbg.contains("reason"));
    }

    #[test]
    fn decision_clone() {
        let d = Decision::deny("reason");
        let d2 = d.clone();
        assert_eq!(d.allowed, d2.allowed);
        assert_eq!(d.reason, d2.reason);
    }

    #[test]
    fn deny_both_read_and_write_same_pattern() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["**/.env"]),
            deny_write: s(&["**/.env"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_write_path(Path::new(".env")).allowed);
    }

    #[test]
    fn read_allowed_but_write_denied_same_path() {
        let e = engine(&PolicyProfile {
            deny_write: s(&["config.json"]),
            ..PolicyProfile::default()
        });
        assert!(e.can_read_path(Path::new("config.json")).allowed);
        assert!(!e.can_write_path(Path::new("config.json")).allowed);
    }

    #[test]
    fn write_allowed_but_read_denied_same_path() {
        let e = engine(&PolicyProfile {
            deny_read: s(&["secret.key"]),
            ..PolicyProfile::default()
        });
        assert!(!e.can_read_path(Path::new("secret.key")).allowed);
        assert!(e.can_write_path(Path::new("secret.key")).allowed);
    }
}

// =========================================================================
// 19. Complex combination scenarios
// =========================================================================

mod complex_combinations {
    use super::*;

    #[test]
    fn strict_lockdown_policy() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["Read"]),
            disallowed_tools: s(&["*"]),
            deny_read: s(&["**/.env", "**/*.key"]),
            deny_write: s(&["**"]),
            ..PolicyProfile::default()
        });
        // All tools denied (deny * overrides allow Read)
        assert!(!e.can_use_tool("Read").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
        // Specific reads denied
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_read_path(Path::new("cert.key")).allowed);
        // All writes denied
        assert!(!e.can_write_path(Path::new("anything.txt")).allowed);
    }

    #[test]
    fn permissive_policy_with_safety_rails() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["*"]),
            disallowed_tools: s(&["Bash", "Shell", "Exec"]),
            deny_read: s(&["**/.env", "**/.env.*", "**/*.key", "**/*.pem"]),
            deny_write: s(&["**/.git/**", "**/node_modules/**"]),
            ..PolicyProfile::default()
        });
        assert!(e.can_use_tool("Read").allowed);
        assert!(e.can_use_tool("Write").allowed);
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_use_tool("Shell").allowed);
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_read_path(Path::new("cert.pem")).allowed);
        assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
        assert!(!e.can_write_path(Path::new(".git/config")).allowed);
        assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
    }

    #[test]
    fn composed_deny_overrides_vs_allow_overrides() {
        let restrictive = PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            ..PolicyProfile::default()
        };
        let permissive = PolicyProfile::default();

        let deny_wins = ComposedEngine::new(
            vec![restrictive.clone(), permissive.clone()],
            PolicyPrecedence::DenyOverrides,
        )
        .unwrap();
        assert!(deny_wins.check_tool("Bash").is_deny());

        let allow_wins = ComposedEngine::new(
            vec![restrictive, permissive],
            PolicyPrecedence::AllowOverrides,
        )
        .unwrap();
        assert!(allow_wins.check_tool("Bash").is_allow());
    }

    #[test]
    fn merged_set_then_engine_enforcement() {
        let mut set = PolicySet::new("org");
        set.add(PolicyProfile {
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["**/.env"]),
            ..PolicyProfile::default()
        });
        set.add(PolicyProfile {
            disallowed_tools: s(&["Shell"]),
            deny_write: s(&["**/.git/**"]),
            ..PolicyProfile::default()
        });
        let merged = set.merge();
        let e = engine(&merged);
        assert!(!e.can_use_tool("Bash").allowed);
        assert!(!e.can_use_tool("Shell").allowed);
        assert!(e.can_use_tool("Read").allowed);
        assert!(!e.can_read_path(Path::new(".env")).allowed);
        assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
        assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    }

    #[test]
    fn auditor_with_complex_policy() {
        let e = engine(&PolicyProfile {
            allowed_tools: s(&["*"]),
            disallowed_tools: s(&["Bash"]),
            deny_read: s(&["**/.env"]),
            deny_write: s(&["**/.git/**"]),
            ..PolicyProfile::default()
        });
        let mut auditor = PolicyAuditor::new(e);
        auditor.check_tool("Read");
        auditor.check_tool("Bash");
        auditor.check_read(".env");
        auditor.check_read("src/lib.rs");
        auditor.check_write(".git/config");
        auditor.check_write("src/lib.rs");

        let summary = auditor.summary();
        assert_eq!(summary.allowed, 3);
        assert_eq!(summary.denied, 3);
        assert_eq!(summary.warned, 0);
        assert_eq!(auditor.entries().len(), 6);
    }

    #[test]
    fn validation_plus_enforcement() {
        let p = PolicyProfile {
            allowed_tools: s(&["Bash"]),
            disallowed_tools: s(&["Bash"]),
            deny_read: vec!["".to_string()],
            ..PolicyProfile::default()
        };
        let warnings = PolicyValidator::validate(&p);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::OverlappingAllowDeny)
        );
        assert!(warnings.iter().any(|w| w.kind == WarningKind::EmptyGlob));

        // Engine still compiles and enforces deny-wins
        let e = engine(&p);
        assert!(!e.can_use_tool("Bash").allowed);
    }

    #[test]
    fn rule_engine_combined_with_policy_engine() {
        let mut re = RuleEngine::new();
        re.add_rule(Rule {
            id: "deny-bash".into(),
            description: "No bash".into(),
            condition: RuleCondition::Pattern("Bash*".into()),
            effect: RuleEffect::Deny,
            priority: 10,
        });
        re.add_rule(Rule {
            id: "allow-read".into(),
            description: "Allow reads".into(),
            condition: RuleCondition::Pattern("Read*".into()),
            effect: RuleEffect::Allow,
            priority: 5,
        });

        assert_eq!(re.evaluate("BashExec"), RuleEffect::Deny);
        assert_eq!(re.evaluate("ReadFile"), RuleEffect::Allow);

        let pe = engine(&PolicyProfile {
            disallowed_tools: s(&["Bash*"]),
            ..PolicyProfile::default()
        });
        // Both engines agree
        assert!(!pe.can_use_tool("BashExec").allowed);
        assert!(pe.can_use_tool("ReadFile").allowed);
    }
}
