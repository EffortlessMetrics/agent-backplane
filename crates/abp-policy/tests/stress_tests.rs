// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stress tests for `abp-policy`.
//!
//! Deterministic correctness-under-load tests — not benchmarks.

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use std::path::Path;

fn engine(policy: PolicyProfile) -> PolicyEngine {
    PolicyEngine::new(&policy).expect("compile policy")
}

// ---------------------------------------------------------------------------
// 1. Policy with 100 tool deny patterns
// ---------------------------------------------------------------------------

#[test]
fn policy_with_100_tool_deny_patterns() {
    let disallowed: Vec<String> = (0..100).map(|i| format!("DangerousTool_{i}*")).collect();
    let e = engine(PolicyProfile {
        disallowed_tools: disallowed,
        ..Default::default()
    });

    // Every denied tool prefix should be blocked.
    for i in 0..100 {
        let tool = format!("DangerousTool_{i}_variant");
        assert!(
            !e.can_use_tool(&tool).allowed,
            "expected deny for {tool}"
        );
    }

    // A tool that matches none of the deny patterns should be allowed.
    assert!(e.can_use_tool("SafeTool").allowed);
    assert!(e.can_use_tool("Read").allowed);
}

// ---------------------------------------------------------------------------
// 2. Policy with 100 read deny patterns
// ---------------------------------------------------------------------------

#[test]
fn policy_with_100_read_deny_patterns() {
    let deny_read: Vec<String> = (0..100).map(|i| format!("secrets/vault_{i}/**")).collect();
    let e = engine(PolicyProfile {
        deny_read,
        ..Default::default()
    });

    // Each denied vault directory should block reads.
    for i in 0..100 {
        let path = format!("secrets/vault_{i}/key.pem");
        assert!(
            !e.can_read_path(Path::new(&path)).allowed,
            "expected deny-read for {path}"
        );
    }

    // A path outside the deny patterns should be allowed.
    assert!(e.can_read_path(Path::new("src/lib.rs")).allowed);
    assert!(e.can_read_path(Path::new("secrets/readme.md")).allowed);
}

// ---------------------------------------------------------------------------
// 3. Policy with 100 write deny patterns
// ---------------------------------------------------------------------------

#[test]
fn policy_with_100_write_deny_patterns() {
    let deny_write: Vec<String> = (0..100)
        .map(|i| format!("protected/zone_{i}/**/*.lock"))
        .collect();
    let e = engine(PolicyProfile {
        deny_write,
        ..Default::default()
    });

    for i in 0..100 {
        let path = format!("protected/zone_{i}/deep/nested/cargo.lock");
        assert!(
            !e.can_write_path(Path::new(&path)).allowed,
            "expected deny-write for {path}"
        );
    }

    // A .lock file outside the protected zones should be allowed.
    assert!(e.can_write_path(Path::new("other/cargo.lock")).allowed);
    // A non-.lock file inside a protected zone should also be allowed.
    assert!(e.can_write_path(Path::new("protected/zone_0/file.rs")).allowed);
}

// ---------------------------------------------------------------------------
// 4. Sequential compilation of many policies
// ---------------------------------------------------------------------------

#[test]
fn sequential_compilation_of_many_policies() {
    let mut engines = Vec::with_capacity(200);

    for i in 0..200 {
        let policy = PolicyProfile {
            allowed_tools: vec![format!("Tool_{i}")],
            disallowed_tools: vec![format!("Banned_{i}")],
            deny_read: vec![format!("read_deny_{i}/**")],
            deny_write: vec![format!("write_deny_{i}/**")],
            ..Default::default()
        };
        engines.push(engine(policy));
    }

    // Spot-check a handful of compiled engines for correctness.
    assert!(engines[0].can_use_tool("Tool_0").allowed);
    assert!(!engines[0].can_use_tool("Banned_0").allowed);
    assert!(!engines[0].can_use_tool("Tool_1").allowed); // not in this engine's allow list
    assert!(!engines[99].can_read_path(Path::new("read_deny_99/secret.txt")).allowed);
    assert!(engines[99].can_read_path(Path::new("read_deny_0/secret.txt")).allowed);
    assert!(!engines[199].can_write_path(Path::new("write_deny_199/out.bin")).allowed);
    assert!(engines[199].can_write_path(Path::new("write_deny_0/out.bin")).allowed);
}

// ---------------------------------------------------------------------------
// 5. Large path set evaluation
// ---------------------------------------------------------------------------

#[test]
fn large_path_set_evaluation() {
    let e = engine(PolicyProfile {
        deny_read: vec![
            "**/.env*".into(),
            "**/secrets/**".into(),
            "**/*.pem".into(),
        ],
        deny_write: vec![
            "**/.git/**".into(),
            "**/node_modules/**".into(),
            "**/*.lock".into(),
        ],
        disallowed_tools: vec!["Exec*".into(), "Shell*".into()],
        ..Default::default()
    });

    let mut read_denied = 0u32;
    let mut write_denied = 0u32;
    let mut tool_denied = 0u32;

    // Evaluate 5 000 paths for read + write.
    for i in 0..5_000 {
        let path = match i % 5 {
            0 => format!("src/module_{i}/lib.rs"),
            1 => format!("secrets/vault/key_{i}.pem"),
            2 => ".env.production".to_string(),
            3 => format!(".git/objects/{i:04x}"),
            _ => format!("node_modules/pkg_{i}/index.js"),
        };
        let p = Path::new(&path);
        if !e.can_read_path(p).allowed {
            read_denied += 1;
        }
        if !e.can_write_path(p).allowed {
            write_denied += 1;
        }
    }

    // Read denials: secrets/** + *.pem (i%5==1) → 1000, .env* (i%5==2) → 1000 = 2000
    assert_eq!(read_denied, 2000);

    // Write denials: .git/** (i%5==3) → 1000, node_modules/** (i%5==4) → 1000 = 2000
    assert_eq!(write_denied, 2000);

    // Evaluate 1 000 tool names.
    for i in 0..1_000 {
        let tool = if i % 3 == 0 {
            format!("Exec_cmd_{i}")
        } else if i % 3 == 1 {
            format!("Shell_run_{i}")
        } else {
            format!("Read_file_{i}")
        };
        if !e.can_use_tool(&tool).allowed {
            tool_denied += 1;
        }
    }

    // Exec* (i%3==0) → 334, Shell* (i%3==1) → 333 = 667
    assert_eq!(tool_denied, 667);
}
