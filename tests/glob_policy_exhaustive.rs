#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive test suite for `abp-glob` and `abp-policy` crates.

use abp_core::{Capability, PolicyProfile};
use abp_glob::{build_globset, IncludeExcludeGlobs, MatchDecision};
use abp_policy::{Decision, PolicyEngine};
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn p(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

fn assert_allowed(d: &Decision) {
    assert!(d.allowed, "expected allowed, got denied: {:?}", d.reason);
}

fn assert_denied(d: &Decision) {
    assert!(!d.allowed, "expected denied, got allowed");
}

/// All Capability variants for exhaustive testing.
fn all_capabilities() -> Vec<Capability> {
    vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
        Capability::FunctionCalling,
        Capability::Vision,
        Capability::Audio,
        Capability::JsonMode,
        Capability::SystemMessage,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
        Capability::CacheControl,
        Capability::BatchMode,
        Capability::Embeddings,
        Capability::ImageGeneration,
    ]
}

// ===========================================================================
// Module 1: IncludeExcludeGlobs — basic behaviour
// ===========================================================================

mod glob_basic {
    use super::*;

    #[test]
    fn empty_patterns_allow_everything() {
        let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
        assert_eq!(g.decide_str(""), MatchDecision::Allowed);
        assert_eq!(g.decide_str("a/b/c/d.txt"), MatchDecision::Allowed);
    }

    #[test]
    fn include_only_gates() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("README.md"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn exclude_only_denies() {
        let g = IncludeExcludeGlobs::new(&[], &p(&["*.log"])).unwrap();
        assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    }

    #[test]
    fn exclude_beats_include() {
        let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["secret/**"])).unwrap();
        assert_eq!(
            g.decide_str("secret/key.pem"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(g.decide_str("public/index.html"), MatchDecision::Allowed);
    }

    #[test]
    fn multiple_includes() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**", "tests/**", "benches/**"]), &[]).unwrap();
        assert_eq!(g.decide_str("src/a.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("tests/t.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("benches/b.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("docs/d.md"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn multiple_excludes() {
        let g = IncludeExcludeGlobs::new(&[], &p(&["*.log", "*.tmp", "*.bak"])).unwrap();
        assert_eq!(g.decide_str("a.log"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("b.tmp"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("c.bak"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("d.rs"), MatchDecision::Allowed);
    }

    #[test]
    fn decide_path_matches_decide_str() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/generated/**"])).unwrap();
        for path in &["src/lib.rs", "src/generated/x.rs", "README.md"] {
            assert_eq!(
                g.decide_str(path),
                g.decide_path(Path::new(path)),
                "mismatch for {path}"
            );
        }
    }
}

// ===========================================================================
// Module 2: Nested directory and complex glob patterns
// ===========================================================================

mod glob_nested {
    use super::*;

    #[test]
    fn double_star_foo_double_star() {
        let g = IncludeExcludeGlobs::new(&p(&["**/foo/**"]), &[]).unwrap();
        assert_eq!(g.decide_str("foo/bar.txt"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("a/foo/bar.txt"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("a/b/foo/c/d.txt"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("bar/baz.txt"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn star_rs_extension() {
        let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &[]).unwrap();
        assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("readme.md"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn exclude_bak_extension() {
        let g = IncludeExcludeGlobs::new(&p(&["**"]), &p(&["*.bak"])).unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("backup.bak"), MatchDecision::DeniedByExclude);
        assert_eq!(
            g.decide_str("deep/path/file.bak"),
            MatchDecision::DeniedByExclude
        );
    }

    #[test]
    fn rs_include_bak_exclude_combined() {
        let g = IncludeExcludeGlobs::new(&p(&["*.rs"]), &p(&["*.bak"])).unwrap();
        assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("readme.md"),
            MatchDecision::DeniedByMissingInclude
        );
        // .bak matches exclude, and exclude is checked first (takes precedence)
        assert_eq!(g.decide_str("file.bak"), MatchDecision::DeniedByExclude);
    }

    #[test]
    fn deeply_nested_paths() {
        let g = IncludeExcludeGlobs::new(&p(&["a/**/z.txt"]), &[]).unwrap();
        assert_eq!(g.decide_str("a/z.txt"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("a/b/z.txt"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("a/b/c/d/e/z.txt"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("b/z.txt"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn question_mark_wildcard() {
        let g = IncludeExcludeGlobs::new(&p(&["file?.txt"]), &[]).unwrap();
        assert_eq!(g.decide_str("file1.txt"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("fileA.txt"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("file10.txt"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn bracket_character_class() {
        let g = IncludeExcludeGlobs::new(&p(&["src/[abc]*.rs"]), &[]).unwrap();
        assert_eq!(g.decide_str("src/alpha.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("src/bravo.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("src/charlie.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("src/delta.rs"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn brace_alternation() {
        let g = IncludeExcludeGlobs::new(&p(&["*.{rs,toml,md}"]), &[]).unwrap();
        assert_eq!(g.decide_str("lib.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("data.json"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn exclude_nested_git_directory() {
        let g = IncludeExcludeGlobs::new(&[], &p(&["**/.git/**"])).unwrap();
        assert_eq!(g.decide_str(".git/config"), MatchDecision::DeniedByExclude);
        assert_eq!(
            g.decide_str(".git/refs/heads/main"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
    }

    #[test]
    fn exclude_hidden_files() {
        let g = IncludeExcludeGlobs::new(&[], &p(&["**/.*"])).unwrap();
        assert_eq!(g.decide_str(".env"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("config/.env"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
    }

    #[test]
    fn multiple_double_star_segments() {
        let g = IncludeExcludeGlobs::new(&p(&["**/src/**/test/**"]), &[]).unwrap();
        assert_eq!(
            g.decide_str("project/src/core/test/unit.rs"),
            MatchDecision::Allowed
        );
        assert_eq!(g.decide_str("src/test/file.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("project/lib/test/unit.rs"),
            MatchDecision::DeniedByMissingInclude
        );
    }
}

// ===========================================================================
// Module 3: Overlapping include/exclude rules
// ===========================================================================

mod glob_overlapping {
    use super::*;

    #[test]
    fn overlapping_include_and_exclude_same_pattern() {
        // exclude always wins
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/**"])).unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByExclude);
    }

    #[test]
    fn narrow_exclude_within_wide_include() {
        let g =
            IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/generated/**", "src/vendor/**"]))
                .unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("src/generated/g.rs"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(
            g.decide_str("src/vendor/v.rs"),
            MatchDecision::DeniedByExclude
        );
    }

    #[test]
    fn multiple_overlapping_includes() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**", "src/core/**"]), &[]).unwrap();
        assert_eq!(g.decide_str("src/core/x.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("src/y.rs"), MatchDecision::Allowed);
    }

    #[test]
    fn extension_vs_directory_overlap() {
        let g = IncludeExcludeGlobs::new(&p(&["**/*.rs"]), &p(&["tests/**"])).unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("tests/test.rs"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(
            g.decide_str("tests/fixtures/data.json"),
            MatchDecision::DeniedByExclude
        );
    }

    #[test]
    fn exclude_superset_of_include() {
        let g = IncludeExcludeGlobs::new(&p(&["src/core/**"]), &p(&["src/**"])).unwrap();
        // src/core/** matches include, but src/** matches exclude first
        assert_eq!(
            g.decide_str("src/core/x.rs"),
            MatchDecision::DeniedByExclude
        );
    }

    #[test]
    fn layered_excludes_multiple_extensions() {
        let g = IncludeExcludeGlobs::new(
            &p(&["**"]),
            &p(&["*.log", "*.tmp", "*.bak", "*.swp", "*.swo"]),
        )
        .unwrap();
        for ext in &["log", "tmp", "bak", "swp", "swo"] {
            assert_eq!(
                g.decide_str(&format!("file.{ext}")),
                MatchDecision::DeniedByExclude,
                "expected {ext} denied"
            );
        }
        assert_eq!(g.decide_str("file.rs"), MatchDecision::Allowed);
    }
}

// ===========================================================================
// Module 4: Path normalization
// ===========================================================================

mod glob_path_normalization {
    use super::*;

    #[test]
    fn forward_slash_paths() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
        assert_eq!(g.decide_str("src/a/b.rs"), MatchDecision::Allowed);
    }

    #[test]
    fn decide_path_with_std_path() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
        assert_eq!(
            g.decide_path(Path::new("src/a/b.rs")),
            MatchDecision::Allowed
        );
    }

    #[test]
    fn empty_path() {
        let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        assert_eq!(g.decide_str(""), MatchDecision::Allowed);
    }

    #[test]
    fn empty_path_with_include() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
        assert_eq!(g.decide_str(""), MatchDecision::DeniedByMissingInclude);
    }

    #[test]
    fn trailing_slash() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &[]).unwrap();
        // "src/" is within the src/** pattern
        assert_eq!(g.decide_str("src/"), MatchDecision::Allowed);
    }

    #[test]
    fn dot_dot_traversal_path() {
        let g = IncludeExcludeGlobs::new(&[], &p(&["**/etc/passwd"])).unwrap();
        assert_eq!(
            g.decide_str("../../etc/passwd"),
            MatchDecision::DeniedByExclude
        );
    }

    #[test]
    fn unicode_paths() {
        let g = IncludeExcludeGlobs::new(&p(&["données/**"]), &[]).unwrap();
        assert_eq!(g.decide_str("données/fichier.txt"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("other/file.txt"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn paths_with_spaces() {
        let g = IncludeExcludeGlobs::new(&p(&["my project/**"]), &[]).unwrap();
        assert_eq!(
            g.decide_str("my project/src/lib.rs"),
            MatchDecision::Allowed
        );
    }

    #[test]
    fn single_dot_path() {
        let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        assert_eq!(g.decide_str("."), MatchDecision::Allowed);
    }
}

// ===========================================================================
// Module 5: Glob compilation errors
// ===========================================================================

mod glob_errors {
    use super::*;

    #[test]
    fn invalid_unclosed_bracket() {
        let err = IncludeExcludeGlobs::new(&p(&["["]), &[]).unwrap_err();
        assert!(
            err.to_string().contains("invalid glob"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn invalid_pattern_in_exclude() {
        let err = IncludeExcludeGlobs::new(&[], &p(&["["])).unwrap_err();
        assert!(err.to_string().contains("invalid glob"));
    }

    #[test]
    fn valid_alongside_invalid_include() {
        let err = IncludeExcludeGlobs::new(&p(&["*.rs", "["]), &[]).unwrap_err();
        assert!(err.to_string().contains("invalid glob"));
    }

    #[test]
    fn valid_alongside_invalid_exclude() {
        let err = IncludeExcludeGlobs::new(&[], &p(&["*.rs", "["])).unwrap_err();
        assert!(err.to_string().contains("invalid glob"));
    }

    #[test]
    fn build_globset_empty_returns_none() {
        assert!(build_globset(&[]).unwrap().is_none());
    }

    #[test]
    fn build_globset_non_empty_returns_some() {
        let gs = build_globset(&p(&["*.rs"])).unwrap();
        assert!(gs.is_some());
    }

    #[test]
    fn build_globset_invalid_returns_err() {
        let err = build_globset(&p(&["["])).unwrap_err();
        assert!(err.to_string().contains("invalid glob"));
    }
}

// ===========================================================================
// Module 6: MatchDecision
// ===========================================================================

mod match_decision {
    use super::*;

    #[test]
    fn allowed_is_allowed() {
        assert!(MatchDecision::Allowed.is_allowed());
    }

    #[test]
    fn denied_by_exclude_not_allowed() {
        assert!(!MatchDecision::DeniedByExclude.is_allowed());
    }

    #[test]
    fn denied_by_missing_include_not_allowed() {
        assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
    }

    #[test]
    fn clone_and_copy() {
        let d = MatchDecision::Allowed;
        let d2 = d;
        let d3 = d.clone();
        assert_eq!(d, d2);
        assert_eq!(d, d3);
    }

    #[test]
    fn debug_format() {
        let dbg = format!("{:?}", MatchDecision::Allowed);
        assert!(dbg.contains("Allowed"));
    }
}

// ===========================================================================
// Module 7: PolicyEngine — tool checks (allow/deny)
// ===========================================================================

mod policy_tool_checks {
    use super::*;

    #[test]
    fn empty_policy_allows_all_tools() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        assert_allowed(&e.can_use_tool("Bash"));
        assert_allowed(&e.can_use_tool("Read"));
        assert_allowed(&e.can_use_tool("Write"));
        assert_allowed(&e.can_use_tool("anything"));
    }

    #[test]
    fn disallow_specific_tool() {
        let policy = PolicyProfile {
            disallowed_tools: p(&["Bash"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_use_tool("Bash"));
        assert_allowed(&e.can_use_tool("Read"));
    }

    #[test]
    fn disallow_with_glob_pattern() {
        let policy = PolicyProfile {
            disallowed_tools: p(&["Bash*"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_use_tool("BashExec"));
        assert_denied(&e.can_use_tool("BashRun"));
        assert_denied(&e.can_use_tool("Bash"));
        assert_allowed(&e.can_use_tool("Read"));
    }

    #[test]
    fn allowlist_blocks_unlisted_tools() {
        let policy = PolicyProfile {
            allowed_tools: p(&["Read", "Grep"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_allowed(&e.can_use_tool("Read"));
        assert_allowed(&e.can_use_tool("Grep"));
        assert_denied(&e.can_use_tool("Bash"));
        assert_denied(&e.can_use_tool("Write"));
    }

    #[test]
    fn wildcard_allowlist_permits_all() {
        let policy = PolicyProfile {
            allowed_tools: p(&["*"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_allowed(&e.can_use_tool("Bash"));
        assert_allowed(&e.can_use_tool("Read"));
        assert_allowed(&e.can_use_tool("anything_at_all"));
    }

    #[test]
    fn deny_beats_wildcard_allow() {
        let policy = PolicyProfile {
            allowed_tools: p(&["*"]),
            disallowed_tools: p(&["Bash"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_use_tool("Bash"));
        assert_allowed(&e.can_use_tool("Read"));
    }

    #[test]
    fn deny_beats_explicit_allow() {
        let policy = PolicyProfile {
            allowed_tools: p(&["Bash", "Read"]),
            disallowed_tools: p(&["Bash"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_use_tool("Bash"));
        assert_allowed(&e.can_use_tool("Read"));
    }

    #[test]
    fn deny_reason_mentions_disallowed() {
        let policy = PolicyProfile {
            disallowed_tools: p(&["Bash"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        let d = e.can_use_tool("Bash");
        assert!(d.reason.as_deref().unwrap().contains("disallowed"));
    }

    #[test]
    fn deny_reason_mentions_not_in_allowlist() {
        let policy = PolicyProfile {
            allowed_tools: p(&["Read"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        let d = e.can_use_tool("Bash");
        assert!(d.reason.as_deref().unwrap().contains("not in allowlist"));
    }

    #[test]
    fn multiple_disallowed_tools() {
        let policy = PolicyProfile {
            disallowed_tools: p(&["Bash", "Exec", "Shell"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_use_tool("Bash"));
        assert_denied(&e.can_use_tool("Exec"));
        assert_denied(&e.can_use_tool("Shell"));
        assert_allowed(&e.can_use_tool("Read"));
    }

    #[test]
    fn glob_allowlist_pattern() {
        let policy = PolicyProfile {
            allowed_tools: p(&["Tool*"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_allowed(&e.can_use_tool("ToolRead"));
        assert_allowed(&e.can_use_tool("ToolWrite"));
        assert_denied(&e.can_use_tool("Bash"));
    }

    #[test]
    fn brace_alternation_in_tool_rules() {
        let policy = PolicyProfile {
            allowed_tools: p(&["{Read,Write,Grep}"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_allowed(&e.can_use_tool("Read"));
        assert_allowed(&e.can_use_tool("Write"));
        assert_allowed(&e.can_use_tool("Grep"));
        assert_denied(&e.can_use_tool("Bash"));
    }
}

// ===========================================================================
// Module 8: PolicyEngine — read path checks
// ===========================================================================

mod policy_read_checks {
    use super::*;

    #[test]
    fn empty_deny_read_allows_all() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        assert_allowed(&e.can_read_path(Path::new("any/file.txt")));
        assert_allowed(&e.can_read_path(Path::new(".env")));
    }

    #[test]
    fn deny_read_specific_file() {
        let policy = PolicyProfile {
            deny_read: p(&["secret.txt"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_read_path(Path::new("secret.txt")));
        assert_allowed(&e.can_read_path(Path::new("public.txt")));
    }

    #[test]
    fn deny_read_glob_pattern() {
        let policy = PolicyProfile {
            deny_read: p(&["**/.env"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_read_path(Path::new(".env")));
        assert_denied(&e.can_read_path(Path::new("config/.env")));
        assert_allowed(&e.can_read_path(Path::new("src/main.rs")));
    }

    #[test]
    fn deny_read_multiple_patterns() {
        let policy = PolicyProfile {
            deny_read: p(&["**/.env", "**/.env.*", "**/id_rsa", "**/*.key"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_read_path(Path::new(".env")));
        assert_denied(&e.can_read_path(Path::new(".env.production")));
        assert_denied(&e.can_read_path(Path::new("ssh/id_rsa")));
        assert_denied(&e.can_read_path(Path::new("certs/server.key")));
        assert_allowed(&e.can_read_path(Path::new("src/lib.rs")));
    }

    #[test]
    fn deny_read_directory_recursive() {
        let policy = PolicyProfile {
            deny_read: p(&["secrets/**"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_read_path(Path::new("secrets/api_key.txt")));
        assert_denied(&e.can_read_path(Path::new("secrets/nested/deep.txt")));
        assert_allowed(&e.can_read_path(Path::new("src/main.rs")));
    }

    #[test]
    fn deny_read_reason_contains_path() {
        let policy = PolicyProfile {
            deny_read: p(&["secret*"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        let d = e.can_read_path(Path::new("secret.txt"));
        assert!(d.reason.as_deref().unwrap().contains("denied"));
    }

    #[test]
    fn deny_read_with_path_traversal() {
        let policy = PolicyProfile {
            deny_read: p(&["**/etc/passwd"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_read_path(Path::new("../../etc/passwd")));
    }

    #[test]
    fn deny_read_extension_wildcard() {
        let policy = PolicyProfile {
            deny_read: p(&["*.{pem,key,cert}"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_read_path(Path::new("server.pem")));
        assert_denied(&e.can_read_path(Path::new("server.key")));
        assert_denied(&e.can_read_path(Path::new("server.cert")));
        assert_allowed(&e.can_read_path(Path::new("server.rs")));
    }
}

// ===========================================================================
// Module 9: PolicyEngine — write path checks
// ===========================================================================

mod policy_write_checks {
    use super::*;

    #[test]
    fn empty_deny_write_allows_all() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        assert_allowed(&e.can_write_path(Path::new("any/file.txt")));
    }

    #[test]
    fn deny_write_specific_file() {
        let policy = PolicyProfile {
            deny_write: p(&["Cargo.lock"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_write_path(Path::new("Cargo.lock")));
        assert_allowed(&e.can_write_path(Path::new("Cargo.toml")));
    }

    #[test]
    fn deny_write_git_directory() {
        let policy = PolicyProfile {
            deny_write: p(&["**/.git/**"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_write_path(Path::new(".git/config")));
        assert_denied(&e.can_write_path(Path::new(".git/refs/heads/main")));
        assert_allowed(&e.can_write_path(Path::new("src/lib.rs")));
    }

    #[test]
    fn deny_write_multiple_patterns() {
        let policy = PolicyProfile {
            deny_write: p(&["**/.git/**", "**/node_modules/**", "*.lock"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_write_path(Path::new(".git/config")));
        assert_denied(&e.can_write_path(Path::new("node_modules/pkg/index.js")));
        assert_denied(&e.can_write_path(Path::new("Cargo.lock")));
        assert_allowed(&e.can_write_path(Path::new("src/main.rs")));
    }

    #[test]
    fn deny_write_deep_nested() {
        let policy = PolicyProfile {
            deny_write: p(&["protected/**"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_write_path(Path::new("protected/a/b/c/d.txt")));
        assert_allowed(&e.can_write_path(Path::new("public/data.txt")));
    }

    #[test]
    fn deny_write_reason_contains_denied() {
        let policy = PolicyProfile {
            deny_write: p(&["locked*"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        let d = e.can_write_path(Path::new("locked.md"));
        assert!(d.reason.as_deref().unwrap().contains("denied"));
    }

    #[test]
    fn deny_write_with_traversal() {
        let policy = PolicyProfile {
            deny_write: p(&["**/.git/**"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_write_path(Path::new("../.git/config")));
    }

    #[test]
    fn deny_write_and_deny_read_independent() {
        let policy = PolicyProfile {
            deny_read: p(&["secret/**"]),
            deny_write: p(&["locked/**"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        // Can write to secret dir (only read denied)
        assert_allowed(&e.can_write_path(Path::new("secret/x.txt")));
        // Can read from locked dir (only write denied)
        assert_allowed(&e.can_read_path(Path::new("locked/y.txt")));
        // Cross-check
        assert_denied(&e.can_read_path(Path::new("secret/x.txt")));
        assert_denied(&e.can_write_path(Path::new("locked/y.txt")));
    }
}

// ===========================================================================
// Module 10: Deny takes precedence over allow
// ===========================================================================

mod deny_precedence {
    use super::*;

    #[test]
    fn tool_deny_beats_tool_allow_exact() {
        let policy = PolicyProfile {
            allowed_tools: p(&["Bash"]),
            disallowed_tools: p(&["Bash"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_use_tool("Bash"));
    }

    #[test]
    fn tool_deny_glob_beats_allow_wildcard() {
        let policy = PolicyProfile {
            allowed_tools: p(&["*"]),
            disallowed_tools: p(&["Bash*", "Shell*"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_use_tool("BashExec"));
        assert_denied(&e.can_use_tool("ShellRun"));
        assert_allowed(&e.can_use_tool("Read"));
    }

    #[test]
    fn glob_exclude_beats_glob_include_on_same_path() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**"]), &p(&["src/**"])).unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::DeniedByExclude);
    }

    #[test]
    fn policy_deny_write_applies_regardless_of_tool_allow() {
        let policy = PolicyProfile {
            allowed_tools: p(&["*"]),
            deny_write: p(&["**/.git/**"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_allowed(&e.can_use_tool("Write"));
        assert_denied(&e.can_write_path(Path::new(".git/config")));
    }

    #[test]
    fn deny_read_applies_regardless_of_tool_allow() {
        let policy = PolicyProfile {
            allowed_tools: p(&["*"]),
            deny_read: p(&["secret/**"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_allowed(&e.can_use_tool("Read"));
        assert_denied(&e.can_read_path(Path::new("secret/key.pem")));
    }
}

// ===========================================================================
// Module 11: Empty patterns (everything allowed)
// ===========================================================================

mod empty_patterns {
    use super::*;

    #[test]
    fn empty_include_empty_exclude_glob() {
        let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        for path in &[
            "",
            "a",
            "a/b",
            "a/b/c/d/e",
            ".hidden",
            "file.rs",
            "deeply/nested/path.txt",
        ] {
            assert_eq!(
                g.decide_str(path),
                MatchDecision::Allowed,
                "expected allowed for '{path}'"
            );
        }
    }

    #[test]
    fn empty_policy_profile_allows_all_tools() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        for tool in &[
            "Bash",
            "Read",
            "Write",
            "Grep",
            "Edit",
            "ToolWebSearch",
            "AnyTool",
            "CustomTool123",
        ] {
            assert_allowed(&e.can_use_tool(tool));
        }
    }

    #[test]
    fn empty_policy_allows_all_read_paths() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        for path in &[
            "src/lib.rs",
            ".env",
            "secret/key.pem",
            "../../../etc/passwd",
        ] {
            assert_allowed(&e.can_read_path(Path::new(path)));
        }
    }

    #[test]
    fn empty_policy_allows_all_write_paths() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        for path in &["src/lib.rs", ".git/config", "node_modules/x"] {
            assert_allowed(&e.can_write_path(Path::new(path)));
        }
    }
}

// ===========================================================================
// Module 12: Complex overlapping include/exclude
// ===========================================================================

mod complex_overlapping {
    use super::*;

    #[test]
    fn project_layout_include_src_tests_exclude_generated_fixtures() {
        let g = IncludeExcludeGlobs::new(
            &p(&["src/**", "tests/**"]),
            &p(&["src/generated/**", "tests/fixtures/**"]),
        )
        .unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("tests/unit.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("src/generated/output.rs"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(
            g.decide_str("tests/fixtures/data.json"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(
            g.decide_str("docs/README.md"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn all_except_hidden_and_temp() {
        let g = IncludeExcludeGlobs::new(
            &p(&["**"]),
            &p(&["**/.*", "**/*.tmp", "**/*.bak", "**/target/**"]),
        )
        .unwrap();
        assert_eq!(g.decide_str("src/main.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str(".env"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("x.tmp"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("y.bak"), MatchDecision::DeniedByExclude);
        assert_eq!(
            g.decide_str("target/debug/bin"),
            MatchDecision::DeniedByExclude
        );
    }

    #[test]
    fn multi_extension_include_with_exclude_subdir() {
        let g = IncludeExcludeGlobs::new(
            &p(&["**/*.rs", "**/*.toml", "**/*.md"]),
            &p(&["target/**", "vendor/**"]),
        )
        .unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("Cargo.toml"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("README.md"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("target/debug/main.rs"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(
            g.decide_str("vendor/crate/lib.rs"),
            MatchDecision::DeniedByExclude
        );
        assert_eq!(
            g.decide_str("data.json"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn security_sensitive_policy() {
        let policy = PolicyProfile {
            allowed_tools: p(&["Read", "Grep", "Glob"]),
            disallowed_tools: p(&["Bash*", "Exec*", "Shell*"]),
            deny_read: p(&["**/.env*", "**/id_rsa*", "**/*.key", "**/*.pem"]),
            deny_write: p(&["**/.git/**", "**/node_modules/**", "**/*.lock"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();

        // Tools
        assert_allowed(&e.can_use_tool("Read"));
        assert_allowed(&e.can_use_tool("Grep"));
        assert_denied(&e.can_use_tool("BashExec"));
        assert_denied(&e.can_use_tool("Exec"));
        assert_denied(&e.can_use_tool("ShellRun"));
        assert_denied(&e.can_use_tool("Write")); // not in allowlist

        // Read paths
        assert_denied(&e.can_read_path(Path::new(".env")));
        assert_denied(&e.can_read_path(Path::new(".env.production")));
        assert_denied(&e.can_read_path(Path::new("home/.ssh/id_rsa")));
        assert_denied(&e.can_read_path(Path::new("certs/server.key")));
        assert_denied(&e.can_read_path(Path::new("certs/server.pem")));
        assert_allowed(&e.can_read_path(Path::new("src/main.rs")));

        // Write paths
        assert_denied(&e.can_write_path(Path::new(".git/config")));
        assert_denied(&e.can_write_path(Path::new("node_modules/x/y")));
        assert_denied(&e.can_write_path(Path::new("Cargo.lock")));
        assert_allowed(&e.can_write_path(Path::new("src/lib.rs")));
    }

    #[test]
    fn overlapping_deny_read_and_deny_write() {
        let policy = PolicyProfile {
            deny_read: p(&["sensitive/**"]),
            deny_write: p(&["sensitive/**"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_read_path(Path::new("sensitive/data.txt")));
        assert_denied(&e.can_write_path(Path::new("sensitive/data.txt")));
        assert_allowed(&e.can_read_path(Path::new("public/data.txt")));
        assert_allowed(&e.can_write_path(Path::new("public/data.txt")));
    }
}

// ===========================================================================
// Module 13: Capability variants against policy
// ===========================================================================

mod capability_policy {
    use super::*;

    #[test]
    fn all_capabilities_exist() {
        let caps = all_capabilities();
        assert!(caps.len() >= 40, "expected at least 40 capabilities");
    }

    #[test]
    fn capability_serializes_to_snake_case() {
        let json = serde_json::to_string(&Capability::ToolRead).unwrap();
        assert_eq!(json, "\"tool_read\"");
    }

    #[test]
    fn capability_streaming_snake_case() {
        let json = serde_json::to_string(&Capability::Streaming).unwrap();
        assert_eq!(json, "\"streaming\"");
    }

    #[test]
    fn capability_tool_bash_snake_case() {
        let json = serde_json::to_string(&Capability::ToolBash).unwrap();
        assert_eq!(json, "\"tool_bash\"");
    }

    #[test]
    fn capability_extended_thinking_snake_case() {
        let json = serde_json::to_string(&Capability::ExtendedThinking).unwrap();
        assert_eq!(json, "\"extended_thinking\"");
    }

    #[test]
    fn capability_structured_output_json_schema_snake_case() {
        let json = serde_json::to_string(&Capability::StructuredOutputJsonSchema).unwrap();
        assert_eq!(json, "\"structured_output_json_schema\"");
    }

    #[test]
    fn capability_roundtrip_all() {
        for cap in all_capabilities() {
            let json = serde_json::to_string(&cap).unwrap();
            let parsed: Capability = serde_json::from_str(&json).unwrap();
            assert_eq!(cap, parsed, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn tool_capabilities_can_be_used_as_tool_names() {
        let tool_caps = vec![
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolEdit,
            Capability::ToolBash,
            Capability::ToolGlob,
            Capability::ToolGrep,
            Capability::ToolWebSearch,
            Capability::ToolWebFetch,
            Capability::ToolAskUser,
        ];

        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        for cap in &tool_caps {
            let name = serde_json::to_string(cap)
                .unwrap()
                .trim_matches('"')
                .to_string();
            assert_allowed(&e.can_use_tool(&name));
        }
    }

    #[test]
    fn deny_specific_capability_tool_names() {
        let policy = PolicyProfile {
            disallowed_tools: p(&["tool_bash", "tool_write"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_use_tool("tool_bash"));
        assert_denied(&e.can_use_tool("tool_write"));
        assert_allowed(&e.can_use_tool("tool_read"));
    }

    #[test]
    fn deny_all_tool_capabilities_via_glob() {
        let policy = PolicyProfile {
            disallowed_tools: p(&["tool_*"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();

        let tool_names = [
            "tool_read",
            "tool_write",
            "tool_edit",
            "tool_bash",
            "tool_glob",
            "tool_grep",
            "tool_web_search",
            "tool_web_fetch",
            "tool_ask_user",
        ];
        for name in &tool_names {
            assert_denied(&e.can_use_tool(name));
        }
        // Non-tool names should still be allowed
        assert_allowed(&e.can_use_tool("streaming"));
        assert_allowed(&e.can_use_tool("vision"));
    }

    #[test]
    fn allowlist_only_tool_capabilities() {
        let policy = PolicyProfile {
            allowed_tools: p(&["tool_*"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_allowed(&e.can_use_tool("tool_read"));
        assert_allowed(&e.can_use_tool("tool_bash"));
        assert_denied(&e.can_use_tool("streaming"));
        assert_denied(&e.can_use_tool("vision"));
    }

    #[test]
    fn all_capability_names_are_non_empty() {
        for cap in all_capabilities() {
            let json = serde_json::to_string(&cap).unwrap();
            let name = json.trim_matches('"');
            assert!(!name.is_empty(), "empty capability name for {:?}", cap);
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "unexpected chars in capability name: {name}"
            );
        }
    }
}

// ===========================================================================
// Module 14: Decision type
// ===========================================================================

mod decision_type {
    use super::*;

    #[test]
    fn decision_allow_has_no_reason() {
        let d = Decision::allow();
        assert!(d.allowed);
        assert!(d.reason.is_none());
    }

    #[test]
    fn decision_deny_has_reason() {
        let d = Decision::deny("not permitted");
        assert!(!d.allowed);
        assert_eq!(d.reason.as_deref(), Some("not permitted"));
    }

    #[test]
    fn decision_deny_with_empty_reason() {
        let d = Decision::deny("");
        assert!(!d.allowed);
        assert_eq!(d.reason.as_deref(), Some(""));
    }

    #[test]
    fn decision_serialization_roundtrip_allow() {
        let d = Decision::allow();
        let json = serde_json::to_string(&d).unwrap();
        let d2: Decision = serde_json::from_str(&json).unwrap();
        assert_eq!(d.allowed, d2.allowed);
        assert_eq!(d.reason, d2.reason);
    }

    #[test]
    fn decision_serialization_roundtrip_deny() {
        let d = Decision::deny("forbidden");
        let json = serde_json::to_string(&d).unwrap();
        let d2: Decision = serde_json::from_str(&json).unwrap();
        assert_eq!(d.allowed, d2.allowed);
        assert_eq!(d.reason, d2.reason);
    }
}

// ===========================================================================
// Module 15: PolicyEngine compilation errors
// ===========================================================================

mod policy_compile_errors {
    use super::*;

    #[test]
    fn invalid_tool_glob_returns_error() {
        let policy = PolicyProfile {
            allowed_tools: p(&["["]),
            ..PolicyProfile::default()
        };
        let err = PolicyEngine::new(&policy).unwrap_err();
        assert!(err.to_string().contains("tool policy"));
    }

    #[test]
    fn invalid_deny_read_glob_returns_error() {
        let policy = PolicyProfile {
            deny_read: p(&["["]),
            ..PolicyProfile::default()
        };
        let err = PolicyEngine::new(&policy).unwrap_err();
        assert!(err.to_string().contains("deny_read"));
    }

    #[test]
    fn invalid_deny_write_glob_returns_error() {
        let policy = PolicyProfile {
            deny_write: p(&["["]),
            ..PolicyProfile::default()
        };
        let err = PolicyEngine::new(&policy).unwrap_err();
        assert!(err.to_string().contains("deny_write"));
    }

    #[test]
    fn invalid_disallowed_tool_glob_returns_error() {
        let policy = PolicyProfile {
            disallowed_tools: p(&["["]),
            ..PolicyProfile::default()
        };
        let err = PolicyEngine::new(&policy).unwrap_err();
        assert!(err.to_string().contains("tool policy"));
    }
}

// ===========================================================================
// Module 16: Edge cases and miscellaneous
// ===========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn single_character_path() {
        let g = IncludeExcludeGlobs::new(&p(&["?"]), &[]).unwrap();
        assert_eq!(g.decide_str("a"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("Z"), MatchDecision::Allowed);
    }

    #[test]
    fn very_long_path() {
        let g = IncludeExcludeGlobs::new(&p(&["**"]), &[]).unwrap();
        let long_path = "a/".repeat(100) + "file.txt";
        assert_eq!(g.decide_str(&long_path), MatchDecision::Allowed);
    }

    #[test]
    fn tool_name_with_special_chars() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        assert_allowed(&e.can_use_tool("tool-with-dashes"));
        assert_allowed(&e.can_use_tool("tool_with_underscores"));
        assert_allowed(&e.can_use_tool("tool.with.dots"));
    }

    #[test]
    fn file_extension_case_sensitivity() {
        let g = IncludeExcludeGlobs::new(&p(&["*.RS"]), &[]).unwrap();
        // globset is case-insensitive on Windows, case-sensitive on Unix
        // we just verify it doesn't panic
        let _ = g.decide_str("main.rs");
        let _ = g.decide_str("main.RS");
    }

    #[test]
    fn exclude_exact_filename() {
        let g = IncludeExcludeGlobs::new(&[], &p(&["Makefile"])).unwrap();
        assert_eq!(g.decide_str("Makefile"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("src/Makefile"), MatchDecision::Allowed);
    }

    #[test]
    fn exclude_any_depth_filename() {
        let g = IncludeExcludeGlobs::new(&[], &p(&["**/Makefile"])).unwrap();
        assert_eq!(g.decide_str("Makefile"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("src/Makefile"), MatchDecision::DeniedByExclude);
        assert_eq!(
            g.decide_str("a/b/c/Makefile"),
            MatchDecision::DeniedByExclude
        );
    }

    #[test]
    fn policy_profile_default_has_empty_vecs() {
        let pp = PolicyProfile::default();
        assert!(pp.allowed_tools.is_empty());
        assert!(pp.disallowed_tools.is_empty());
        assert!(pp.deny_read.is_empty());
        assert!(pp.deny_write.is_empty());
        assert!(pp.allow_network.is_empty());
        assert!(pp.deny_network.is_empty());
        assert!(pp.require_approval_for.is_empty());
    }

    #[test]
    fn policy_network_fields_stored_correctly() {
        let policy = PolicyProfile {
            allow_network: p(&["*.example.com"]),
            deny_network: p(&["evil.example.com"]),
            ..PolicyProfile::default()
        };
        let _e = PolicyEngine::new(&policy).unwrap();
        assert_eq!(policy.allow_network, vec!["*.example.com"]);
        assert_eq!(policy.deny_network, vec!["evil.example.com"]);
    }

    #[test]
    fn policy_require_approval_stored_correctly() {
        let policy = PolicyProfile {
            require_approval_for: p(&["Bash", "DeleteFile"]),
            ..PolicyProfile::default()
        };
        let _e = PolicyEngine::new(&policy).unwrap();
        assert_eq!(policy.require_approval_for, vec!["Bash", "DeleteFile"]);
    }

    #[test]
    fn glob_with_negated_character_class() {
        let g = IncludeExcludeGlobs::new(&p(&["[!a]*"]), &[]).unwrap();
        assert_eq!(g.decide_str("alpha"), MatchDecision::DeniedByMissingInclude);
        assert_eq!(g.decide_str("beta"), MatchDecision::Allowed);
    }

    #[test]
    fn include_multiple_brace_patterns() {
        let g = IncludeExcludeGlobs::new(&p(&["src/**/*.{rs,toml}", "docs/**/*.md"]), &[]).unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("src/Cargo.toml"), MatchDecision::Allowed);
        assert_eq!(g.decide_str("docs/guide.md"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("src/data.json"),
            MatchDecision::DeniedByMissingInclude
        );
    }
}

// ===========================================================================
// Module 17: Batch capability-as-tool-name tests
// ===========================================================================

mod capability_tool_batch {
    use super::*;

    fn cap_name(c: &Capability) -> String {
        serde_json::to_string(c)
            .unwrap()
            .trim_matches('"')
            .to_string()
    }

    #[test]
    fn empty_policy_allows_all_capability_names_as_tools() {
        let e = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        for cap in all_capabilities() {
            let name = cap_name(&cap);
            assert_allowed(&e.can_use_tool(&name));
        }
    }

    #[test]
    fn deny_all_capabilities_as_tools() {
        let policy = PolicyProfile {
            disallowed_tools: p(&["*"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        for cap in all_capabilities() {
            let name = cap_name(&cap);
            assert_denied(&e.can_use_tool(&name));
        }
    }

    #[test]
    fn allowlist_single_capability_deny_rest() {
        let policy = PolicyProfile {
            allowed_tools: p(&["streaming"]),
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_allowed(&e.can_use_tool("streaming"));
        assert_denied(&e.can_use_tool("tool_read"));
        assert_denied(&e.can_use_tool("vision"));
    }

    #[test]
    fn selective_deny_capabilities() {
        let dangerous = p(&["tool_bash", "tool_write", "tool_edit", "code_execution"]);
        let policy = PolicyProfile {
            disallowed_tools: dangerous,
            ..PolicyProfile::default()
        };
        let e = PolicyEngine::new(&policy).unwrap();
        assert_denied(&e.can_use_tool("tool_bash"));
        assert_denied(&e.can_use_tool("tool_write"));
        assert_denied(&e.can_use_tool("tool_edit"));
        assert_denied(&e.can_use_tool("code_execution"));
        assert_allowed(&e.can_use_tool("tool_read"));
        assert_allowed(&e.can_use_tool("streaming"));
        assert_allowed(&e.can_use_tool("vision"));
    }
}
