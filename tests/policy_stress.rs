// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stress tests and edge cases for the policy engine.

use std::path::Path;
use std::time::Instant;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use abp_policy::audit::{AuditSummary, PolicyAuditor};
use abp_policy::compose::{
    ComposedEngine, PolicyPrecedence, PolicySet, PolicyValidator, WarningKind,
};
use abp_policy::rules::{Rule, RuleCondition, RuleEffect, RuleEngine};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn s(v: &str) -> String {
    v.to_string()
}

fn strings(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| s(x)).collect()
}

// ===========================================================================
// 1. Large policy profiles (100+ rules)
// ===========================================================================

#[test]
fn large_disallowed_tools_list() {
    let tools: Vec<String> = (0..200).map(|i| format!("Tool_{i}")).collect();
    let policy = PolicyProfile {
        disallowed_tools: tools.clone(),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile 200 deny rules");

    for tool in &tools {
        assert!(!engine.can_use_tool(tool).allowed, "should deny {tool}");
    }
    assert!(engine.can_use_tool("Unlisted").allowed);
}

#[test]
fn large_deny_write_patterns() {
    let patterns: Vec<String> = (0..150).map(|i| format!("dir_{i}/**")).collect();
    let policy = PolicyProfile {
        deny_write: patterns.clone(),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile 150 deny_write patterns");

    for i in 0..150 {
        let path = format!("dir_{i}/file.txt");
        assert!(
            !engine.can_write_path(Path::new(&path)).allowed,
            "should deny write to {path}"
        );
    }
    assert!(engine.can_write_path(Path::new("safe/file.txt")).allowed);
}

#[test]
fn large_deny_read_patterns() {
    let patterns: Vec<String> = (0..120).map(|i| format!("**/secret_{i}.*")).collect();
    let policy = PolicyProfile {
        deny_read: patterns,
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile 120 deny_read patterns");

    assert!(!engine.can_read_path(Path::new("secret_0.txt")).allowed);
    assert!(
        !engine
            .can_read_path(Path::new("a/b/secret_119.json"))
            .allowed
    );
    assert!(engine.can_read_path(Path::new("public.txt")).allowed);
}

#[test]
fn large_allowed_tools_list() {
    let tools: Vec<String> = (0..300).map(|i| format!("Allowed_{i}")).collect();
    let policy = PolicyProfile {
        allowed_tools: tools.clone(),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile 300 allow rules");

    for tool in &tools {
        assert!(engine.can_use_tool(tool).allowed);
    }
    assert!(!engine.can_use_tool("NotInList").allowed);
}

// ===========================================================================
// 2. Complex glob patterns with nested includes/excludes
// ===========================================================================

#[test]
fn deeply_nested_glob_patterns() {
    let policy = PolicyProfile {
        deny_write: strings(&[
            "**/.git/**",
            "**/node_modules/**",
            "**/.env*",
            "**/dist/**",
            "**/build/**",
            "**/*.lock",
            "**/target/**",
        ]),
        deny_read: strings(&[
            "**/.env",
            "**/.env.*",
            "**/id_rsa",
            "**/id_ed25519",
            "**/*.pem",
            "**/*.key",
        ]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile complex globs");

    // Deeply nested .git path
    assert!(
        !engine
            .can_write_path(Path::new("a/b/c/.git/objects/pack"))
            .allowed
    );
    // node_modules at various depths
    assert!(
        !engine
            .can_write_path(Path::new("node_modules/lodash/index.js"))
            .allowed
    );
    assert!(
        !engine
            .can_write_path(Path::new("packages/web/node_modules/react/index.js"))
            .allowed
    );
    // Secrets at depth
    assert!(!engine.can_read_path(Path::new("home/.ssh/id_rsa")).allowed);
    assert!(
        !engine
            .can_read_path(Path::new("deep/nested/path/to/.env"))
            .allowed
    );
    assert!(!engine.can_read_path(Path::new("certs/server.pem")).allowed);
    // Normal files allowed
    assert!(engine.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn brace_expansion_style_patterns() {
    // globset supports {a,b} alternation
    let policy = PolicyProfile {
        deny_write: strings(&["**/*.{lock,bak,tmp}", "**/._*"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile brace patterns");

    assert!(!engine.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!engine.can_write_path(Path::new("data.bak")).allowed);
    assert!(!engine.can_write_path(Path::new("scratch.tmp")).allowed);
    assert!(!engine.can_write_path(Path::new("dir/._DS_Store")).allowed);
    assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn question_mark_glob_pattern() {
    let policy = PolicyProfile {
        deny_read: strings(&["secret?.txt"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile ? pattern");

    assert!(!engine.can_read_path(Path::new("secret1.txt")).allowed);
    assert!(!engine.can_read_path(Path::new("secretX.txt")).allowed);
    // Two chars → no match (? matches exactly one character)
    assert!(engine.can_read_path(Path::new("secret12.txt")).allowed);
    assert!(engine.can_read_path(Path::new("secret.txt")).allowed);
}

#[test]
fn character_class_glob_pattern() {
    let policy = PolicyProfile {
        deny_write: strings(&["**/log[0-9].txt"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile char class pattern");

    assert!(!engine.can_write_path(Path::new("log0.txt")).allowed);
    assert!(!engine.can_write_path(Path::new("logs/log9.txt")).allowed);
    assert!(engine.can_write_path(Path::new("logA.txt")).allowed);
}

// ===========================================================================
// 3. Policy compilation performance (benchmark-style)
// ===========================================================================

#[test]
fn compilation_performance_many_patterns() {
    let patterns: Vec<String> = (0..500).map(|i| format!("**/dir_{i}/**")).collect();
    let policy = PolicyProfile {
        deny_write: patterns,
        ..PolicyProfile::default()
    };

    let start = Instant::now();
    let engine = PolicyEngine::new(&policy).expect("compile 500 patterns");
    let compile_elapsed = start.elapsed();

    // Compilation should complete in a reasonable time (< 5 seconds)
    assert!(
        compile_elapsed.as_secs() < 5,
        "compilation took too long: {compile_elapsed:?}"
    );

    // Evaluation should also be fast
    let start = Instant::now();
    for i in 0..1000 {
        let path = format!("dir_{}/file_{}.txt", i % 500, i);
        let _ = engine.can_write_path(Path::new(&path));
    }
    let eval_elapsed = start.elapsed();

    assert!(
        eval_elapsed.as_secs() < 5,
        "1000 evaluations took too long: {eval_elapsed:?}"
    );
}

#[test]
fn evaluation_throughput_many_checks() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["Read", "Write", "Grep", "Glob", "Edit"]),
        disallowed_tools: strings(&["Bash*", "Shell*", "Exec*"]),
        deny_read: strings(&["**/.env*", "**/*.key", "**/*.pem"]),
        deny_write: strings(&["**/.git/**", "**/node_modules/**", "**/*.lock"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile policy");

    let start = Instant::now();
    let iterations = 10_000;
    for _ in 0..iterations {
        let _ = engine.can_use_tool("Read");
        let _ = engine.can_use_tool("BashExec");
        let _ = engine.can_read_path(Path::new("src/lib.rs"));
        let _ = engine.can_read_path(Path::new(".env.production"));
        let _ = engine.can_write_path(Path::new("src/main.rs"));
        let _ = engine.can_write_path(Path::new(".git/config"));
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs() < 10,
        "{} evaluations took too long: {elapsed:?}",
        iterations * 6
    );
}

// ===========================================================================
// 4. Policy decision caching / repeated evaluation consistency
// ===========================================================================

#[test]
fn repeated_evaluations_are_consistent() {
    let policy = PolicyProfile {
        disallowed_tools: strings(&["Bash"]),
        deny_read: strings(&["**/.env"]),
        deny_write: strings(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile");

    // Run the same checks many times, results must be identical
    for _ in 0..1000 {
        assert!(!engine.can_use_tool("Bash").allowed);
        assert!(engine.can_use_tool("Read").allowed);
        assert!(!engine.can_read_path(Path::new(".env")).allowed);
        assert!(engine.can_read_path(Path::new("ok.txt")).allowed);
        assert!(!engine.can_write_path(Path::new(".git/HEAD")).allowed);
        assert!(engine.can_write_path(Path::new("src/lib.rs")).allowed);
    }
}

#[test]
fn clone_engine_produces_same_decisions() {
    let policy = PolicyProfile {
        disallowed_tools: strings(&["Dangerous*"]),
        deny_write: strings(&["**/protected/**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile");
    let cloned = engine.clone();

    let tools = ["DangerousExec", "Read", "Write", "DangerousBash"];
    let paths = ["protected/secret.txt", "src/lib.rs", "protected/a/b.txt"];

    for tool in &tools {
        assert_eq!(
            engine.can_use_tool(tool).allowed,
            cloned.can_use_tool(tool).allowed,
            "mismatch for tool {tool}"
        );
    }
    for path in &paths {
        assert_eq!(
            engine.can_write_path(Path::new(path)).allowed,
            cloned.can_write_path(Path::new(path)).allowed,
            "mismatch for path {path}"
        );
    }
}

// ===========================================================================
// 5. Conflicting rules (allow and deny same path/tool)
// ===========================================================================

#[test]
fn deny_always_beats_allow_for_tools() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["Bash", "Write", "*"]),
        disallowed_tools: strings(&["Bash", "Write"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile conflicting tool rules");

    // Deny wins even though they are also in the allowlist
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Write").allowed);
    // Wildcard allow still works for non-denied tools
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);
}

#[test]
fn wildcard_deny_overrides_wildcard_allow() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["*"]),
        disallowed_tools: strings(&["*"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile wildcard conflict");

    // Deny takes precedence
    assert!(!engine.can_use_tool("AnyTool").allowed);
    assert!(!engine.can_use_tool("Read").allowed);
}

#[test]
fn overlapping_glob_deny_write_patterns() {
    let policy = PolicyProfile {
        deny_write: strings(&["src/**", "src/main.rs"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile overlapping patterns");

    // Both patterns deny, result is still denied
    assert!(!engine.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(!engine.can_write_path(Path::new("src/lib.rs")).allowed);
    assert!(engine.can_write_path(Path::new("tests/test.rs")).allowed);
}

// ===========================================================================
// 6. Priority/precedence edge cases (RuleEngine + ComposedEngine)
// ===========================================================================

#[test]
fn rule_engine_higher_priority_wins() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("allow-all"),
        description: s("Allow everything"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    engine.add_rule(Rule {
        id: s("deny-bash"),
        description: s("Deny Bash tool"),
        condition: RuleCondition::Pattern(s("Bash*")),
        effect: RuleEffect::Deny,
        priority: 10,
    });

    assert_eq!(engine.evaluate("BashExec"), RuleEffect::Deny);
    assert_eq!(engine.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_same_priority_first_wins() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("deny-first"),
        description: s("Deny first"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Deny,
        priority: 5,
    });
    engine.add_rule(Rule {
        id: s("allow-second"),
        description: s("Allow second"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 5,
    });

    // Both match at same priority; max_by_key returns the last max, so "allow-second" wins
    // (Rust's max_by_key picks the later element on tie)
    let result = engine.evaluate("anything");
    assert_eq!(result, RuleEffect::Allow);
}

#[test]
fn rule_engine_no_matching_rules_defaults_to_allow() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("deny-bash"),
        description: s("Deny Bash"),
        condition: RuleCondition::Pattern(s("Bash")),
        effect: RuleEffect::Deny,
        priority: 10,
    });

    assert_eq!(engine.evaluate("Read"), RuleEffect::Allow);
}

#[test]
fn rule_engine_100_rules_priority_ordering() {
    let mut engine = RuleEngine::new();
    for i in 0..100u32 {
        engine.add_rule(Rule {
            id: format!("rule-{i}"),
            description: format!("Rule at priority {i}"),
            condition: RuleCondition::Always,
            effect: if i == 99 {
                RuleEffect::Deny
            } else {
                RuleEffect::Allow
            },
            priority: i,
        });
    }

    // Highest priority (99) is Deny, so it wins
    assert_eq!(engine.evaluate("anything"), RuleEffect::Deny);
}

#[test]
fn composed_engine_deny_overrides() {
    let permissive = PolicyProfile {
        allowed_tools: strings(&["*"]),
        ..PolicyProfile::default()
    };
    let restrictive = PolicyProfile {
        disallowed_tools: strings(&["Bash"]),
        ..PolicyProfile::default()
    };

    let engine = ComposedEngine::new(
        vec![permissive, restrictive],
        PolicyPrecedence::DenyOverrides,
    )
    .expect("compile composed");

    assert!(engine.check_tool("Bash").is_deny());
    assert!(engine.check_tool("Read").is_allow());
}

#[test]
fn composed_engine_allow_overrides() {
    let permissive = PolicyProfile {
        allowed_tools: strings(&["*"]),
        ..PolicyProfile::default()
    };
    let restrictive = PolicyProfile {
        disallowed_tools: strings(&["Bash"]),
        ..PolicyProfile::default()
    };

    let engine = ComposedEngine::new(
        vec![permissive, restrictive],
        PolicyPrecedence::AllowOverrides,
    )
    .expect("compile composed");

    // Allow overrides: the permissive policy allows Bash, so it wins
    assert!(engine.check_tool("Bash").is_allow());
}

#[test]
fn composed_engine_first_applicable() {
    let first = PolicyProfile {
        disallowed_tools: strings(&["Bash"]),
        ..PolicyProfile::default()
    };
    let second = PolicyProfile {
        allowed_tools: strings(&["*"]),
        ..PolicyProfile::default()
    };

    let engine = ComposedEngine::new(vec![first, second], PolicyPrecedence::FirstApplicable)
        .expect("compile composed");

    // First profile denies Bash → that's the first non-abstain decision
    assert!(engine.check_tool("Bash").is_deny());
}

#[test]
fn composed_engine_empty_policies_abstain() {
    let engine = ComposedEngine::new(vec![], PolicyPrecedence::DenyOverrides)
        .expect("compile empty composed");

    assert!(engine.check_tool("Anything").is_abstain());
    assert!(engine.check_read("any/path").is_abstain());
    assert!(engine.check_write("any/path").is_abstain());
}

// ===========================================================================
// 7. Real-world SDK policy scenarios
// ===========================================================================

#[test]
fn claude_code_read_only_mode() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["Read", "Grep", "Glob", "View", "ListFiles"]),
        disallowed_tools: strings(&["Write", "Edit", "Create", "Bash*", "Shell*", "Execute*"]),
        deny_write: strings(&["**"]),
        deny_read: strings(&[
            "**/.env*",
            "**/*.key",
            "**/*.pem",
            "**/id_rsa",
            "**/id_ed25519",
        ]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile claude read-only policy");

    // Read-only tools allowed
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Grep").allowed);
    assert!(engine.can_use_tool("Glob").allowed);

    // Write/exec tools denied
    assert!(!engine.can_use_tool("Write").allowed);
    assert!(!engine.can_use_tool("Edit").allowed);
    assert!(!engine.can_use_tool("BashExec").allowed);
    assert!(!engine.can_use_tool("ShellRun").allowed);

    // All writes denied
    assert!(!engine.can_write_path(Path::new("src/main.rs")).allowed);
    assert!(!engine.can_write_path(Path::new("README.md")).allowed);

    // Reads mostly allowed except secrets
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("certs/server.pem")).allowed);
}

#[test]
fn copilot_workspace_sandboxed_mode() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["*"]),
        disallowed_tools: strings(&["Bash", "Shell", "Execute", "Terminal"]),
        deny_write: strings(&[
            "**/.git/**",
            "**/node_modules/**",
            "**/*.lock",
            "**/package-lock.json",
        ]),
        deny_read: strings(&["**/.env*", "**/*.secret"]),
        require_approval_for: strings(&["DeleteFile", "RenameFile"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile copilot sandbox policy");

    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Terminal").allowed);
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(
        !engine
            .can_write_path(Path::new("node_modules/lodash/index.js"))
            .allowed
    );
    assert!(engine.can_write_path(Path::new("src/index.ts")).allowed);
    assert!(!engine.can_read_path(Path::new(".env.local")).allowed);
    assert!(engine.can_read_path(Path::new("src/index.ts")).allowed);
}

#[test]
fn ci_pipeline_minimal_permissions() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["Read", "Grep", "Test", "Build", "Lint"]),
        disallowed_tools: vec![],
        deny_write: strings(&["**"]),
        deny_read: strings(&["**/.env*", "**/credentials*", "**/*.secret"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile CI policy");

    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Build").allowed);
    assert!(!engine.can_use_tool("Bash").allowed); // not in allowlist
    assert!(!engine.can_write_path(Path::new("anything")).allowed);
    assert!(engine.can_read_path(Path::new("src/main.rs")).allowed);
    assert!(!engine.can_read_path(Path::new("credentials.json")).allowed);
}

#[test]
fn monorepo_scoped_access() {
    // Simulate restricting an agent to only one package in a monorepo
    let policy = PolicyProfile {
        deny_write: strings(&[
            "packages/auth/**",
            "packages/billing/**",
            "packages/infra/**",
            "**/.github/**",
            "**/terraform/**",
        ]),
        deny_read: strings(&["packages/auth/secrets/**", "packages/billing/keys/**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile monorepo policy");

    // Can write to the allowed package
    assert!(
        engine
            .can_write_path(Path::new("packages/frontend/src/App.tsx"))
            .allowed
    );
    // Cannot write to restricted packages
    assert!(
        !engine
            .can_write_path(Path::new("packages/auth/src/login.ts"))
            .allowed
    );
    assert!(
        !engine
            .can_write_path(Path::new("packages/billing/src/charge.ts"))
            .allowed
    );
    assert!(
        !engine
            .can_write_path(Path::new(".github/workflows/ci.yml"))
            .allowed
    );
    // Can read most things
    assert!(
        engine
            .can_read_path(Path::new("packages/auth/src/login.ts"))
            .allowed
    );
    // Cannot read secrets
    assert!(
        !engine
            .can_read_path(Path::new("packages/auth/secrets/key.pem"))
            .allowed
    );
}

// ===========================================================================
// 8. Policy serialization/deserialization roundtrip
// ===========================================================================

#[test]
fn policy_profile_serde_roundtrip() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["Read", "Write", "Grep"]),
        disallowed_tools: strings(&["Bash*", "Shell*"]),
        deny_read: strings(&["**/.env*", "**/*.key"]),
        deny_write: strings(&["**/.git/**", "**/node_modules/**"]),
        allow_network: strings(&["*.example.com"]),
        deny_network: strings(&["evil.example.com"]),
        require_approval_for: strings(&["DeleteFile"]),
    };

    let json = serde_json::to_string_pretty(&policy).expect("serialize");
    let deserialized: PolicyProfile = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(policy.allowed_tools, deserialized.allowed_tools);
    assert_eq!(policy.disallowed_tools, deserialized.disallowed_tools);
    assert_eq!(policy.deny_read, deserialized.deny_read);
    assert_eq!(policy.deny_write, deserialized.deny_write);
    assert_eq!(policy.allow_network, deserialized.allow_network);
    assert_eq!(policy.deny_network, deserialized.deny_network);
    assert_eq!(
        policy.require_approval_for,
        deserialized.require_approval_for
    );

    // The deserialized policy should produce the same engine behavior
    let engine1 = PolicyEngine::new(&policy).expect("compile original");
    let engine2 = PolicyEngine::new(&deserialized).expect("compile deserialized");

    let tools = ["Read", "Bash", "BashExec", "Write", "ShellRun", "Grep"];
    for tool in &tools {
        assert_eq!(
            engine1.can_use_tool(tool).allowed,
            engine2.can_use_tool(tool).allowed,
            "tool decision mismatch after roundtrip for {tool}"
        );
    }

    let paths = [".env", ".env.production", "src/main.rs", ".git/config"];
    for path in &paths {
        assert_eq!(
            engine1.can_read_path(Path::new(path)).allowed,
            engine2.can_read_path(Path::new(path)).allowed,
            "read decision mismatch after roundtrip for {path}"
        );
        assert_eq!(
            engine1.can_write_path(Path::new(path)).allowed,
            engine2.can_write_path(Path::new(path)).allowed,
            "write decision mismatch after roundtrip for {path}"
        );
    }
}

#[test]
fn default_policy_serde_roundtrip() {
    let policy = PolicyProfile::default();
    let json = serde_json::to_string(&policy).expect("serialize default");
    let deserialized: PolicyProfile = serde_json::from_str(&json).expect("deserialize default");

    assert!(deserialized.allowed_tools.is_empty());
    assert!(deserialized.disallowed_tools.is_empty());
    assert!(deserialized.deny_read.is_empty());
    assert!(deserialized.deny_write.is_empty());
    assert!(deserialized.allow_network.is_empty());
    assert!(deserialized.deny_network.is_empty());
    assert!(deserialized.require_approval_for.is_empty());
}

#[test]
fn rule_engine_serde_roundtrip() {
    let rule = Rule {
        id: s("r1"),
        description: s("Deny dangerous tools"),
        condition: RuleCondition::And(vec![
            RuleCondition::Pattern(s("Bash*")),
            RuleCondition::Not(Box::new(RuleCondition::Pattern(s("BashLite")))),
        ]),
        effect: RuleEffect::Deny,
        priority: 10,
    };

    let json = serde_json::to_string_pretty(&rule).expect("serialize rule");
    let deserialized: Rule = serde_json::from_str(&json).expect("deserialize rule");

    assert_eq!(deserialized.id, "r1");
    assert_eq!(deserialized.priority, 10);
    assert_eq!(deserialized.effect, RuleEffect::Deny);

    // Verify condition logic survives roundtrip
    assert!(deserialized.condition.matches("BashExec"));
    assert!(!deserialized.condition.matches("BashLite"));
    assert!(!deserialized.condition.matches("Read"));
}

#[test]
fn audit_entry_serde_roundtrip() {
    let policy = PolicyProfile {
        disallowed_tools: strings(&["Bash"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile");
    let mut auditor = PolicyAuditor::new(engine);

    auditor.check_tool("Bash");
    auditor.check_tool("Read");
    auditor.check_read("src/lib.rs");

    let json = serde_json::to_string(auditor.entries()).expect("serialize entries");
    let _deserialized: Vec<serde_json::Value> =
        serde_json::from_str(&json).expect("deserialize entries");

    assert_eq!(auditor.entries().len(), 3);
}

// ===========================================================================
// 9. Empty/null policy edge cases
// ===========================================================================

#[test]
fn empty_policy_is_fully_permissive() {
    let engine = PolicyEngine::new(&PolicyProfile::default()).expect("compile empty");

    assert!(engine.can_use_tool("").allowed);
    assert!(engine.can_use_tool("Anything").allowed);
    assert!(engine.can_read_path(Path::new("")).allowed);
    assert!(engine.can_read_path(Path::new("any/path")).allowed);
    assert!(engine.can_write_path(Path::new("")).allowed);
    assert!(engine.can_write_path(Path::new("any/path")).allowed);
}

#[test]
fn empty_string_tool_name() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["Read"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile");

    // Empty string is not in allowlist
    assert!(!engine.can_use_tool("").allowed);
}

#[test]
fn empty_allowed_tools_with_disallowed() {
    // No allowlist but some denylisted tools
    let policy = PolicyProfile {
        disallowed_tools: strings(&["Bash"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile");

    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
}

#[test]
fn only_deny_lists_no_allow_lists() {
    let policy = PolicyProfile {
        deny_read: strings(&["**/.secret"]),
        deny_write: strings(&["**/readonly/**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile deny-only");

    assert!(engine.can_use_tool("Anything").allowed);
    assert!(!engine.can_read_path(Path::new(".secret")).allowed);
    assert!(engine.can_read_path(Path::new("ok.txt")).allowed);
    assert!(
        !engine
            .can_write_path(Path::new("readonly/file.txt"))
            .allowed
    );
    assert!(
        engine
            .can_write_path(Path::new("writable/file.txt"))
            .allowed
    );
}

#[test]
fn unicode_tool_names_and_paths() {
    let policy = PolicyProfile {
        disallowed_tools: strings(&["ツール*"]),
        deny_read: strings(&["**/données/**"]),
        deny_write: strings(&["**/résultats/**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile unicode policy");

    assert!(!engine.can_use_tool("ツールX").allowed);
    assert!(engine.can_use_tool("Tool").allowed);
    assert!(
        !engine
            .can_read_path(Path::new("données/fichier.txt"))
            .allowed
    );
    assert!(
        !engine
            .can_write_path(Path::new("résultats/output.csv"))
            .allowed
    );
}

#[test]
fn special_characters_in_paths() {
    let policy = PolicyProfile {
        deny_write: strings(&["**/spaces in name/**", "**/file with (parens).txt"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile special char policy");

    assert!(
        !engine
            .can_write_path(Path::new("spaces in name/file.txt"))
            .allowed
    );
    assert!(
        !engine
            .can_write_path(Path::new("dir/file with (parens).txt"))
            .allowed
    );
    assert!(engine.can_write_path(Path::new("normal/file.txt")).allowed);
}

// ===========================================================================
// 10. Policy composition (combining multiple profiles)
// ===========================================================================

#[test]
fn policy_set_merge_unions_all_lists() {
    let mut set = PolicySet::new("combined");

    set.add(PolicyProfile {
        allowed_tools: strings(&["Read"]),
        disallowed_tools: strings(&["Bash"]),
        deny_read: strings(&["**/.env"]),
        deny_write: strings(&["**/.git/**"]),
        ..PolicyProfile::default()
    });
    set.add(PolicyProfile {
        allowed_tools: strings(&["Write"]),
        disallowed_tools: strings(&["Shell"]),
        deny_read: strings(&["**/*.key"]),
        deny_write: strings(&["**/node_modules/**"]),
        ..PolicyProfile::default()
    });

    let merged = set.merge();
    assert_eq!(set.name(), "combined");

    let engine = PolicyEngine::new(&merged).expect("compile merged");

    // Both profiles' tools are in the merged allowlist
    assert!(engine.can_use_tool("Read").allowed);
    assert!(engine.can_use_tool("Write").allowed);
    // Both profiles' denials are enforced
    assert!(!engine.can_use_tool("Bash").allowed);
    assert!(!engine.can_use_tool("Shell").allowed);
    // Not in either allowlist
    assert!(!engine.can_use_tool("Grep").allowed);

    // Deny-read from both profiles
    assert!(!engine.can_read_path(Path::new(".env")).allowed);
    assert!(!engine.can_read_path(Path::new("certs/server.key")).allowed);

    // Deny-write from both profiles
    assert!(!engine.can_write_path(Path::new(".git/config")).allowed);
    assert!(
        !engine
            .can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
}

#[test]
fn policy_set_merge_deduplicates() {
    let mut set = PolicySet::new("dedup");
    let profile = PolicyProfile {
        allowed_tools: strings(&["Read", "Write"]),
        disallowed_tools: strings(&["Bash"]),
        ..PolicyProfile::default()
    };

    // Add the same profile twice
    set.add(profile.clone());
    set.add(profile);

    let merged = set.merge();
    assert_eq!(merged.allowed_tools.len(), 2); // deduped
    assert_eq!(merged.disallowed_tools.len(), 1); // deduped
}

#[test]
fn composed_engine_many_profiles() {
    let profiles: Vec<PolicyProfile> = (0..20)
        .map(|i| PolicyProfile {
            disallowed_tools: vec![format!("Tool_{i}")],
            deny_write: vec![format!("dir_{i}/**")],
            ..PolicyProfile::default()
        })
        .collect();

    let engine = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides)
        .expect("compile 20 profiles");

    for i in 0..20 {
        assert!(
            engine.check_tool(&format!("Tool_{i}")).is_deny(),
            "Tool_{i} should be denied"
        );
        assert!(
            engine.check_write(&format!("dir_{i}/file.txt")).is_deny(),
            "dir_{i} write should be denied"
        );
    }
    assert!(engine.check_tool("SafeTool").is_allow());
    assert!(engine.check_write("safe/file.txt").is_allow());
}

#[test]
fn composed_engine_conflicting_profiles_deny_overrides() {
    let allow_all = PolicyProfile {
        allowed_tools: strings(&["*"]),
        ..PolicyProfile::default()
    };
    let deny_specific = PolicyProfile {
        disallowed_tools: strings(&["Bash"]),
        ..PolicyProfile::default()
    };

    let engine = ComposedEngine::new(
        vec![allow_all, deny_specific],
        PolicyPrecedence::DenyOverrides,
    )
    .expect("compile");

    assert!(engine.check_tool("Bash").is_deny());
    assert!(engine.check_tool("Read").is_allow());
}

// ===========================================================================
// Additional edge cases: validator, auditor, and rule conditions
// ===========================================================================

#[test]
fn validator_detects_overlapping_allow_deny() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["Bash"]),
        disallowed_tools: strings(&["Bash"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&policy);

    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::OverlappingAllowDeny),
        "should detect overlapping tool rules"
    );
}

#[test]
fn validator_detects_empty_globs() {
    let policy = PolicyProfile {
        allowed_tools: vec![s("")],
        deny_read: vec![s("")],
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&policy);

    let empty_count = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::EmptyGlob)
        .count();
    assert!(
        empty_count >= 2,
        "should detect empty globs in at least 2 fields"
    );
}

#[test]
fn validator_detects_unreachable_rules() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["Read", "Write"]),
        disallowed_tools: strings(&["*"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&policy);

    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule),
        "should detect unreachable allowed tools"
    );
}

#[test]
fn validator_catch_all_deny_read() {
    let policy = PolicyProfile {
        deny_read: strings(&["**"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&policy);

    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_read"))
    );
}

#[test]
fn validator_catch_all_deny_write() {
    let policy = PolicyProfile {
        deny_write: strings(&["**/*"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&policy);

    assert!(
        warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnreachableRule && w.message.contains("deny_write"))
    );
}

#[test]
fn validator_clean_policy_no_warnings() {
    let policy = PolicyProfile {
        allowed_tools: strings(&["Read", "Write"]),
        disallowed_tools: strings(&["Bash"]),
        deny_read: strings(&["**/.env"]),
        deny_write: strings(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let warnings = PolicyValidator::validate(&policy);
    assert!(
        warnings.is_empty(),
        "clean policy should have no warnings: {warnings:?}"
    );
}

#[test]
fn auditor_tracks_all_decisions() {
    let policy = PolicyProfile {
        disallowed_tools: strings(&["Bash"]),
        deny_read: strings(&["**/.env"]),
        deny_write: strings(&["**/.git/**"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile");
    let mut auditor = PolicyAuditor::new(engine);

    // Perform a mix of checks
    auditor.check_tool("Read");
    auditor.check_tool("Bash");
    auditor.check_read("src/lib.rs");
    auditor.check_read(".env");
    auditor.check_write("src/main.rs");
    auditor.check_write(".git/config");

    assert_eq!(auditor.entries().len(), 6);
    assert_eq!(auditor.allowed_count(), 3);
    assert_eq!(auditor.denied_count(), 3);

    let summary = auditor.summary();
    assert_eq!(
        summary,
        AuditSummary {
            allowed: 3,
            denied: 3,
            warned: 0,
        }
    );
}

#[test]
fn auditor_high_volume() {
    let policy = PolicyProfile {
        disallowed_tools: strings(&["Dangerous*"]),
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).expect("compile");
    let mut auditor = PolicyAuditor::new(engine);

    for i in 0..1000 {
        if i % 2 == 0 {
            auditor.check_tool("SafeTool");
        } else {
            auditor.check_tool("DangerousExec");
        }
    }

    assert_eq!(auditor.entries().len(), 1000);
    assert_eq!(auditor.allowed_count(), 500);
    assert_eq!(auditor.denied_count(), 500);
}

#[test]
fn rule_condition_complex_and_or_not() {
    // (Pattern("Bash*") OR Pattern("Shell*")) AND NOT Pattern("BashLite")
    let condition = RuleCondition::And(vec![
        RuleCondition::Or(vec![
            RuleCondition::Pattern(s("Bash*")),
            RuleCondition::Pattern(s("Shell*")),
        ]),
        RuleCondition::Not(Box::new(RuleCondition::Pattern(s("BashLite")))),
    ]);

    assert!(condition.matches("BashExec"));
    assert!(condition.matches("ShellRun"));
    assert!(!condition.matches("BashLite")); // excluded by NOT
    assert!(!condition.matches("Read")); // doesn't match OR
}

#[test]
fn rule_condition_deeply_nested() {
    // Build a deeply nested condition tree
    let mut condition = RuleCondition::Pattern(s("target"));
    for _ in 0..20 {
        condition = RuleCondition::Not(Box::new(condition));
    }
    // 20 NOTs → even number → matches "target"
    assert!(condition.matches("target"));
    // But not other things
    assert!(!condition.matches("other"));
}

#[test]
fn rule_condition_always_and_never() {
    assert!(RuleCondition::Always.matches("anything"));
    assert!(RuleCondition::Always.matches(""));
    assert!(!RuleCondition::Never.matches("anything"));
    assert!(!RuleCondition::Never.matches(""));
}

#[test]
fn rule_condition_empty_and_or() {
    // Empty AND → all() on empty iterator → true
    assert!(RuleCondition::And(vec![]).matches("anything"));
    // Empty OR → any() on empty iterator → false
    assert!(!RuleCondition::Or(vec![]).matches("anything"));
}

#[test]
fn rule_engine_throttle_and_log_effects() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("log-reads"),
        description: s("Log all read operations"),
        condition: RuleCondition::Pattern(s("Read*")),
        effect: RuleEffect::Log,
        priority: 1,
    });
    engine.add_rule(Rule {
        id: s("throttle-writes"),
        description: s("Throttle write operations"),
        condition: RuleCondition::Pattern(s("Write*")),
        effect: RuleEffect::Throttle { max: 100 },
        priority: 1,
    });

    assert_eq!(engine.evaluate("ReadFile"), RuleEffect::Log);
    assert_eq!(
        engine.evaluate("WriteFile"),
        RuleEffect::Throttle { max: 100 }
    );
    assert_eq!(engine.evaluate("Unknown"), RuleEffect::Allow);
}

#[test]
fn rule_engine_remove_rule() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("r1"),
        description: s("Deny Bash"),
        condition: RuleCondition::Pattern(s("Bash")),
        effect: RuleEffect::Deny,
        priority: 10,
    });

    assert_eq!(engine.evaluate("Bash"), RuleEffect::Deny);
    assert_eq!(engine.rule_count(), 1);

    engine.remove_rule("r1");
    assert_eq!(engine.evaluate("Bash"), RuleEffect::Allow);
    assert_eq!(engine.rule_count(), 0);

    // Removing non-existent rule is a no-op
    engine.remove_rule("nonexistent");
    assert_eq!(engine.rule_count(), 0);
}

#[test]
fn rule_engine_evaluate_all() {
    let mut engine = RuleEngine::new();
    engine.add_rule(Rule {
        id: s("r1"),
        description: s("Allow all"),
        condition: RuleCondition::Always,
        effect: RuleEffect::Allow,
        priority: 1,
    });
    engine.add_rule(Rule {
        id: s("r2"),
        description: s("Deny Bash"),
        condition: RuleCondition::Pattern(s("Bash")),
        effect: RuleEffect::Deny,
        priority: 10,
    });

    let evals = engine.evaluate_all("Bash");
    assert_eq!(evals.len(), 2);
    assert!(evals[0].matched); // r1: Always matches
    assert!(evals[1].matched); // r2: Pattern matches

    let evals = engine.evaluate_all("Read");
    assert_eq!(evals.len(), 2);
    assert!(evals[0].matched); // r1: Always matches
    assert!(!evals[1].matched); // r2: Pattern doesn't match
}

// ===========================================================================
// Stress: many policies composed, large rule engines
// ===========================================================================

#[test]
fn stress_composed_engine_50_profiles() {
    let profiles: Vec<PolicyProfile> = (0..50)
        .map(|i| {
            let mut p = PolicyProfile::default();
            p.disallowed_tools.push(format!("Tool_{i}"));
            p.deny_write.push(format!("project_{i}/**"));
            p.deny_read.push(format!("secret_{i}/**"));
            p
        })
        .collect();

    let engine = ComposedEngine::new(profiles, PolicyPrecedence::DenyOverrides)
        .expect("compile 50 profiles");

    // Spot-check a sampling
    assert!(engine.check_tool("Tool_0").is_deny());
    assert!(engine.check_tool("Tool_49").is_deny());
    assert!(engine.check_tool("SafeTool").is_allow());
    assert!(engine.check_write("project_25/src/main.rs").is_deny());
    assert!(engine.check_read("secret_10/data.txt").is_deny());
    assert!(engine.check_read("public/data.txt").is_allow());
}

#[test]
fn stress_rule_engine_500_rules() {
    let mut engine = RuleEngine::new();
    for i in 0..500u32 {
        engine.add_rule(Rule {
            id: format!("rule-{i}"),
            description: format!("Rule {i}"),
            condition: RuleCondition::Pattern(format!("resource_{i}")),
            effect: if i % 2 == 0 {
                RuleEffect::Allow
            } else {
                RuleEffect::Deny
            },
            priority: i,
        });
    }

    assert_eq!(engine.rule_count(), 500);

    // resource_499 matches rule-499 (Deny, priority 499 — highest matching)
    assert_eq!(engine.evaluate("resource_499"), RuleEffect::Deny);
    // resource_498 matches rule-498 (Allow, priority 498)
    assert_eq!(engine.evaluate("resource_498"), RuleEffect::Allow);
    // Non-matching resource defaults to Allow
    assert_eq!(engine.evaluate("no_match"), RuleEffect::Allow);
}

#[test]
fn policy_set_merge_many_profiles() {
    let mut set = PolicySet::new("large");
    for i in 0..100 {
        set.add(PolicyProfile {
            disallowed_tools: vec![format!("Tool_{i}")],
            deny_write: vec![format!("dir_{i}/**")],
            ..PolicyProfile::default()
        });
    }

    let merged = set.merge();
    assert_eq!(merged.disallowed_tools.len(), 100);
    assert_eq!(merged.deny_write.len(), 100);

    let engine = PolicyEngine::new(&merged).expect("compile merged 100 profiles");
    assert!(!engine.can_use_tool("Tool_0").allowed);
    assert!(!engine.can_use_tool("Tool_99").allowed);
    assert!(engine.can_use_tool("SafeTool").allowed);
}
