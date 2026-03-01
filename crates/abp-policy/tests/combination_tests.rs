// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive policy combination and interaction tests for `abp-policy`.

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn engine(policy: PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(&policy).expect("compile policy")
}

fn s(v: &str) -> String {
    v.to_string()
}

// ===========================================================================
// 1. Tool policy combinations
// ===========================================================================

#[test]
fn tool_allow_all_wildcard_permits_any_tool() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("*")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("SomeObscureTool").allowed);
}

#[test]
fn tool_empty_lists_permit_everything() {
    let e = engine(PolicyProfile::default());
    assert!(e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn tool_deny_specific_blocks_only_that_tool() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Write").allowed);
}

#[test]
fn tool_allow_specific_and_deny_specific_deny_wins() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Bash"), s("Read")],
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    // Deny takes precedence even when explicitly allowed.
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    // Not in allowlist → denied.
    assert!(!e.can_use_tool("Write").allowed);
}

#[test]
fn tool_allow_wildcard_and_deny_specific_deny_still_blocks() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("DangerousTool")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("DangerousTool").allowed);
    assert!(e.can_use_tool("SafeTool").allowed);
}

#[test]
fn tool_multiple_deny_patterns() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash*"), s("Shell*"), s("Exec")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("BashExec").allowed);
    assert!(!e.can_use_tool("ShellRun").allowed);
    assert!(!e.can_use_tool("Exec").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

#[test]
fn tool_allowlist_restricts_to_listed_tools_only() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read"), s("Grep")],
        ..Default::default()
    });
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
}

#[test]
fn tool_deny_glob_pattern_blocks_family() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: vec![s("File*")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("FileRead").allowed);
    assert!(!e.can_use_tool("FileWrite").allowed);
    assert!(!e.can_use_tool("FileDelete").allowed);
    assert!(e.can_use_tool("Grep").allowed);
}

#[test]
fn tool_multiple_allow_patterns_union() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read*"), s("List*")],
        ..Default::default()
    });
    assert!(e.can_use_tool("ReadFile").allowed);
    assert!(e.can_use_tool("ListDir").allowed);
    assert!(!e.can_use_tool("Write").allowed);
}

// ===========================================================================
// 2. Path policy combinations
// ===========================================================================

#[test]
fn path_deny_read_with_glob_blocks_secret_allows_others() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("secret/**")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("secret/key.pem")).allowed);
    assert!(!e.can_read_path(Path::new("secret/deep/nested.txt")).allowed);
    assert!(e.can_read_path(Path::new("public/index.html")).allowed);
}

#[test]
fn path_deny_write_config_reads_still_allowed() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("config/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("config/app.toml")).allowed);
    assert!(!e.can_write_path(Path::new("config/nested/db.yaml")).allowed);
    // Reading the same paths is NOT blocked.
    assert!(e.can_read_path(Path::new("config/app.toml")).allowed);
    assert!(e.can_read_path(Path::new("config/nested/db.yaml")).allowed);
}

#[test]
fn path_nested_deny_narrower_pattern() {
    // Deny deeper path (/a/b/c/**) but allow parent (/a/b/*).
    let e = engine(PolicyProfile {
        deny_write: vec![s("a/b/c/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("a/b/c/file.txt")).allowed);
    assert!(!e.can_write_path(Path::new("a/b/c/d/e.txt")).allowed);
    // Sibling path not blocked.
    assert!(e.can_write_path(Path::new("a/b/file.txt")).allowed);
    assert!(e.can_write_path(Path::new("a/b/d/file.txt")).allowed);
}

#[test]
fn path_double_star_recursive_matching() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/.secret")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new(".secret")).allowed);
    assert!(!e.can_read_path(Path::new("a/.secret")).allowed);
    assert!(!e.can_read_path(Path::new("a/b/c/.secret")).allowed);
    assert!(e.can_read_path(Path::new("a/b/c/public")).allowed);
}

#[test]
fn path_exact_filename_vs_glob_pattern() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("Makefile")],
        deny_write: vec![s("*.lock")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("Makefile")).allowed);
    // Glob *.lock matches any .lock file.
    assert!(!e.can_write_path(Path::new("Cargo.lock")).allowed);
    assert!(!e.can_write_path(Path::new("yarn.lock")).allowed);
    assert!(e.can_write_path(Path::new("Cargo.toml")).allowed);
}

#[test]
fn path_deny_read_and_write_separate_patterns() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("credentials/**")],
        deny_write: vec![s("deploy/**")],
        ..Default::default()
    });
    // Read denied only for credentials.
    assert!(!e.can_read_path(Path::new("credentials/token")).allowed);
    assert!(e.can_read_path(Path::new("deploy/script.sh")).allowed);
    // Write denied only for deploy.
    assert!(!e.can_write_path(Path::new("deploy/script.sh")).allowed);
    assert!(e.can_write_path(Path::new("credentials/token")).allowed);
}

#[test]
fn path_multiple_deny_read_patterns_union() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("**/.env"), s("**/.env.*"), s("**/id_rsa"), s("*.key")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(!e.can_read_path(Path::new("cfg/.env")).allowed);
    assert!(!e.can_read_path(Path::new(".env.local")).allowed);
    assert!(!e.can_read_path(Path::new("ssh/id_rsa")).allowed);
    assert!(!e.can_read_path(Path::new("server.key")).allowed);
    assert!(e.can_read_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn path_multiple_deny_write_patterns_union() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/.git/**"), s("**/node_modules/**"), s("dist/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new(".git/config")).allowed);
    assert!(
        !e.can_write_path(Path::new("node_modules/pkg/index.js"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("dist/bundle.js")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn path_deeply_nested_deny() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("a/b/c/d/e/**")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("a/b/c/d/e/f.txt")).allowed);
    assert!(e.can_write_path(Path::new("a/b/c/d/file.txt")).allowed);
}

#[test]
fn path_extension_glob() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/*.exe"), s("**/*.dll")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("bin/app.exe")).allowed);
    assert!(!e.can_write_path(Path::new("lib/mod.dll")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

// ===========================================================================
// 3. Mixed policies (tool + path combinations)
// ===========================================================================

#[test]
fn mixed_tool_and_path_restrictions() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read"), s("Write"), s("Grep")],
        disallowed_tools: vec![s("Write")],
        deny_read: vec![s("**/.env")],
        deny_write: vec![s("**/locked/**")],
        ..Default::default()
    });
    // Tool checks.
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(e.can_use_tool("Grep").allowed);
    // Path checks.
    assert!(!e.can_read_path(Path::new(".env")).allowed);
    assert!(e.can_read_path(Path::new("README.md")).allowed);
    assert!(!e.can_write_path(Path::new("locked/data.txt")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn mixed_empty_allow_with_deny_patterns() {
    // Empty allowed_tools means "backend default" (no restriction from allow side).
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash"), s("Shell")],
        deny_read: vec![s("secret/**")],
        deny_write: vec![s("**/.git/**")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("Bash").allowed);
    assert!(!e.can_use_tool("Shell").allowed);
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_read_path(Path::new("secret/key")).allowed);
    assert!(e.can_read_path(Path::new("public/page.html")).allowed);
    assert!(!e.can_write_path(Path::new(".git/HEAD")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn extremely_restrictive_policy() {
    // Allowlist only "Read", deny everything else via patterns.
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read")],
        disallowed_tools: vec![],
        deny_read: vec![s("**")],
        deny_write: vec![s("**")],
        ..Default::default()
    });
    // Only Read tool is allowed.
    assert!(e.can_use_tool("Read").allowed);
    assert!(!e.can_use_tool("Write").allowed);
    assert!(!e.can_use_tool("Bash").allowed);
    // All paths denied.
    assert!(!e.can_read_path(Path::new("any/file.txt")).allowed);
    assert!(!e.can_write_path(Path::new("any/file.txt")).allowed);
}

#[test]
fn many_patterns_performance() {
    // Create a policy with many patterns — should compile and evaluate quickly.
    let deny_read: Vec<String> = (0..200).map(|i| format!("secret_{i}/**")).collect();
    let deny_write: Vec<String> = (0..200).map(|i| format!("locked_{i}/**")).collect();
    let disallowed: Vec<String> = (0..100).map(|i| format!("Tool_{i}")).collect();

    let e = engine(PolicyProfile {
        allowed_tools: vec![s("*")],
        disallowed_tools: disallowed,
        deny_read,
        deny_write,
        ..Default::default()
    });

    // Spot-check denied.
    assert!(!e.can_use_tool("Tool_42").allowed);
    assert!(
        !e.can_read_path(Path::new("secret_99/deep/file.txt"))
            .allowed
    );
    assert!(!e.can_write_path(Path::new("locked_150/file.txt")).allowed);

    // Spot-check allowed.
    assert!(e.can_use_tool("SafeTool").allowed);
    assert!(e.can_read_path(Path::new("public/index.html")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn overlapping_deny_read_and_deny_write_on_same_path() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("vault/**")],
        deny_write: vec![s("vault/**")],
        ..Default::default()
    });
    assert!(!e.can_read_path(Path::new("vault/secrets.json")).allowed);
    assert!(!e.can_write_path(Path::new("vault/secrets.json")).allowed);
    // Outside vault is fine.
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(e.can_write_path(Path::new("src/lib.rs")).allowed);
}

#[test]
fn deny_reasons_contain_path_info() {
    let e = engine(PolicyProfile {
        deny_read: vec![s("secret/**")],
        deny_write: vec![s("locked/**")],
        ..Default::default()
    });
    let rd = e.can_read_path(Path::new("secret/key.pem"));
    assert!(!rd.allowed);
    assert!(
        rd.reason.as_deref().unwrap().contains("secret/key.pem"),
        "reason should mention the path: {:?}",
        rd.reason
    );

    let wd = e.can_write_path(Path::new("locked/data.bin"));
    assert!(!wd.allowed);
    assert!(
        wd.reason.as_deref().unwrap().contains("locked/data.bin"),
        "reason should mention the path: {:?}",
        wd.reason
    );
}

#[test]
fn deny_reasons_contain_tool_name() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Bash")],
        ..Default::default()
    });
    let d = e.can_use_tool("Bash");
    assert!(!d.allowed);
    assert!(
        d.reason.as_deref().unwrap().contains("Bash"),
        "reason should mention the tool: {:?}",
        d.reason
    );
}

#[test]
fn tool_not_in_allowlist_reason() {
    let e = engine(PolicyProfile {
        allowed_tools: vec![s("Read")],
        ..Default::default()
    });
    let d = e.can_use_tool("Write");
    assert!(!d.allowed);
    assert!(
        d.reason.as_deref().unwrap().contains("allowlist"),
        "reason should mention allowlist: {:?}",
        d.reason
    );
}

#[test]
fn allow_decision_has_no_reason() {
    let e = engine(PolicyProfile::default());
    let d = e.can_use_tool("Anything");
    assert!(d.allowed);
    assert!(d.reason.is_none());
}

#[test]
fn single_star_vs_double_star_in_path() {
    // globset's default Glob treats `*` as matching path separators,
    // so `config/*` also matches nested paths. Use `**` for explicit
    // recursive intent and verify both behave equivalently here.
    let e = engine(PolicyProfile {
        deny_write: vec![s("config/*")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("config/app.toml")).allowed);
    assert!(!e.can_write_path(Path::new("config/sub/app.toml")).allowed);

    // Outside the `config` prefix is still allowed.
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}

#[test]
fn question_mark_glob_in_tool_deny() {
    let e = engine(PolicyProfile {
        disallowed_tools: vec![s("Run?")],
        ..Default::default()
    });
    assert!(!e.can_use_tool("RunX").allowed);
    assert!(!e.can_use_tool("RunZ").allowed);
    // "Run" is only 3 chars, ? requires exactly one more.
    assert!(e.can_use_tool("Run").allowed);
    // "RunAB" has two extra chars, doesn't match Run?.
    assert!(e.can_use_tool("RunAB").allowed);
}

#[test]
fn brace_glob_in_deny_write() {
    let e = engine(PolicyProfile {
        deny_write: vec![s("**/*.{exe,dll,so}")],
        ..Default::default()
    });
    assert!(!e.can_write_path(Path::new("bin/app.exe")).allowed);
    assert!(!e.can_write_path(Path::new("lib/mod.dll")).allowed);
    assert!(!e.can_write_path(Path::new("lib/mod.so")).allowed);
    assert!(e.can_write_path(Path::new("src/main.rs")).allowed);
}
