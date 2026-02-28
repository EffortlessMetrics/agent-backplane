// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the audit module.

use abp_core::PolicyProfile;
use abp_policy::audit::{AuditEntry, AuditSummary, PolicyAuditor, PolicyDecision};
use abp_policy::PolicyEngine;

fn permissive_engine() -> PolicyEngine {
    PolicyEngine::new(&PolicyProfile::default()).unwrap()
}

fn restrictive_engine() -> PolicyEngine {
    PolicyEngine::new(&PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_read: vec!["secret*".into()],
        deny_write: vec!["locked*".into()],
        ..PolicyProfile::default()
    })
    .unwrap()
}

#[test]
fn auditor_records_allow_decisions() {
    let mut auditor = PolicyAuditor::new(permissive_engine());
    let d = auditor.check_tool("Read");
    assert_eq!(d, PolicyDecision::Allow);
    assert_eq!(auditor.entries().len(), 1);
    assert_eq!(auditor.entries()[0].action, "tool");
    assert_eq!(auditor.entries()[0].resource, "Read");
}

#[test]
fn auditor_records_deny_decisions() {
    let mut auditor = PolicyAuditor::new(restrictive_engine());
    let d = auditor.check_tool("Bash");
    assert!(matches!(d, PolicyDecision::Deny { .. }));
    assert_eq!(auditor.denied_count(), 1);
}

#[test]
fn mixed_decisions_audit_trail() {
    let mut auditor = PolicyAuditor::new(restrictive_engine());
    auditor.check_tool("Read"); // allow (empty allowlist = allow all except denied)
    auditor.check_tool("Bash"); // deny
    auditor.check_read("public.txt"); // allow
    auditor.check_read("secret.txt"); // deny
    assert_eq!(auditor.entries().len(), 4);
    assert_eq!(auditor.allowed_count(), 2);
    assert_eq!(auditor.denied_count(), 2);
}

#[test]
fn summary_counts_are_accurate() {
    let mut auditor = PolicyAuditor::new(restrictive_engine());
    auditor.check_tool("Read");
    auditor.check_tool("Bash");
    auditor.check_read("secret.txt");
    auditor.check_write("locked.md");
    auditor.check_write("ok.txt");
    assert_eq!(
        auditor.summary(),
        AuditSummary {
            allowed: 2,
            denied: 3,
            warned: 0,
        }
    );
}

#[test]
fn empty_auditor_has_zero_counts() {
    let auditor = PolicyAuditor::new(permissive_engine());
    assert_eq!(auditor.entries().len(), 0);
    assert_eq!(auditor.allowed_count(), 0);
    assert_eq!(auditor.denied_count(), 0);
    assert_eq!(
        auditor.summary(),
        AuditSummary {
            allowed: 0,
            denied: 0,
            warned: 0,
        }
    );
}

#[test]
fn tool_check_audit() {
    let mut auditor = PolicyAuditor::new(restrictive_engine());
    auditor.check_tool("Bash");
    let entry = &auditor.entries()[0];
    assert_eq!(entry.action, "tool");
    assert_eq!(entry.resource, "Bash");
    assert!(matches!(entry.decision, PolicyDecision::Deny { .. }));
}

#[test]
fn read_path_audit() {
    let mut auditor = PolicyAuditor::new(restrictive_engine());
    auditor.check_read("secret.key");
    let entry = &auditor.entries()[0];
    assert_eq!(entry.action, "read");
    assert_eq!(entry.resource, "secret.key");
    assert!(matches!(entry.decision, PolicyDecision::Deny { .. }));
}

#[test]
fn write_path_audit() {
    let mut auditor = PolicyAuditor::new(restrictive_engine());
    auditor.check_write("locked.dat");
    let entry = &auditor.entries()[0];
    assert_eq!(entry.action, "write");
    assert_eq!(entry.resource, "locked.dat");
    assert!(matches!(entry.decision, PolicyDecision::Deny { .. }));
}

#[test]
fn multiple_sequential_checks() {
    let mut auditor = PolicyAuditor::new(permissive_engine());
    for i in 0..20 {
        auditor.check_tool(&format!("Tool{i}"));
    }
    assert_eq!(auditor.entries().len(), 20);
    assert_eq!(auditor.allowed_count(), 20);
}

#[test]
fn entries_preserve_order() {
    let mut auditor = PolicyAuditor::new(permissive_engine());
    auditor.check_tool("A");
    auditor.check_read("B");
    auditor.check_write("C");
    let resources: Vec<&str> = auditor.entries().iter().map(|e| e.resource.as_str()).collect();
    assert_eq!(resources, vec!["A", "B", "C"]);
}

#[test]
fn serde_roundtrip_policy_decision() {
    let cases = vec![
        PolicyDecision::Allow,
        PolicyDecision::Deny {
            reason: "forbidden".into(),
        },
        PolicyDecision::Warn {
            reason: "careful".into(),
        },
    ];
    for original in &cases {
        let json = serde_json::to_string(original).unwrap();
        let restored: PolicyDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(&restored, original);
    }
}

#[test]
fn serde_roundtrip_audit_entry() {
    let mut auditor = PolicyAuditor::new(permissive_engine());
    auditor.check_tool("Read");
    let entry = &auditor.entries()[0];
    let json = serde_json::to_string(entry).unwrap();
    let restored: AuditEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.action, entry.action);
    assert_eq!(restored.resource, entry.resource);
    assert_eq!(restored.decision, entry.decision);
    assert_eq!(restored.timestamp, entry.timestamp);
}
