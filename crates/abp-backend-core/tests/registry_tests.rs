//! Tests for [`BackendRegistry`], [`BackendHealth`], [`BackendMetadata`], and [`RateLimit`].

use abp_backend_core::health::{BackendHealth, HealthStatus};
use abp_backend_core::metadata::{BackendMetadata, RateLimit};
use abp_backend_core::registry::BackendRegistry;
use chrono::Utc;

fn sample_metadata(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        name: name.to_string(),
        dialect: dialect.to_string(),
        version: "1.0.0".to_string(),
        max_tokens: Some(4096),
        supports_streaming: true,
        supports_tools: true,
        rate_limit: None,
    }
}

// ── 1. Empty registry ──────────────────────────────────────────────────

#[test]
fn empty_registry_is_empty() {
    let reg = BackendRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn empty_registry_list_returns_empty() {
    let reg = BackendRegistry::new();
    assert!(reg.list().is_empty());
}

#[test]
fn empty_registry_healthy_backends_returns_empty() {
    let reg = BackendRegistry::new();
    assert!(reg.healthy_backends().is_empty());
}

#[test]
fn empty_registry_by_dialect_returns_empty() {
    let reg = BackendRegistry::new();
    assert!(reg.by_dialect("openai").is_empty());
}

#[test]
fn empty_registry_get_metadata_returns_none() {
    let reg = BackendRegistry::new();
    assert!(reg.metadata("nonexistent").is_none());
}

#[test]
fn empty_registry_get_health_returns_none() {
    let reg = BackendRegistry::new();
    assert!(reg.health("nonexistent").is_none());
}

// ── 2. Registration and retrieval ──────────────────────────────────────

#[test]
fn register_and_retrieve_metadata() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    let m = reg.metadata("gpt4").expect("metadata present");
    assert_eq!(m.name, "gpt4");
    assert_eq!(m.dialect, "openai");
}

#[test]
fn register_creates_default_health() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    let h = reg.health("gpt4").expect("health present");
    assert_eq!(h.status, HealthStatus::Unknown);
    assert_eq!(h.consecutive_failures, 0);
}

#[test]
fn contains_returns_true_after_register() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    assert!(reg.contains("gpt4"));
}

#[test]
fn contains_returns_false_for_unknown() {
    let reg = BackendRegistry::new();
    assert!(!reg.contains("gpt4"));
}

#[test]
fn len_after_registrations() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.register_with_metadata("b", sample_metadata("b", "anthropic"));
    assert_eq!(reg.len(), 2);
}

#[test]
fn list_returns_sorted_names() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("zeta", sample_metadata("zeta", "openai"));
    reg.register_with_metadata("alpha", sample_metadata("alpha", "openai"));
    assert_eq!(reg.list(), vec!["alpha", "zeta"]);
}

// ── 3. Health status transitions ───────────────────────────────────────

#[test]
fn update_health_changes_status() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    reg.update_health(
        "gpt4",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    assert_eq!(reg.health("gpt4").unwrap().status, HealthStatus::Healthy);
}

#[test]
fn health_transitions_healthy_to_degraded() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    reg.update_health(
        "gpt4",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    reg.update_health(
        "gpt4",
        BackendHealth {
            status: HealthStatus::Degraded,
            error_rate: 0.3,
            consecutive_failures: 2,
            ..BackendHealth::default()
        },
    );
    let h = reg.health("gpt4").unwrap();
    assert_eq!(h.status, HealthStatus::Degraded);
    assert!((h.error_rate - 0.3).abs() < f64::EPSILON);
    assert_eq!(h.consecutive_failures, 2);
}

#[test]
fn health_transitions_to_unhealthy() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    reg.update_health(
        "gpt4",
        BackendHealth {
            status: HealthStatus::Unhealthy,
            consecutive_failures: 10,
            error_rate: 1.0,
            ..BackendHealth::default()
        },
    );
    let h = reg.health("gpt4").unwrap();
    assert_eq!(h.status, HealthStatus::Unhealthy);
}

#[test]
fn update_health_records_latency() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    reg.update_health(
        "gpt4",
        BackendHealth {
            status: HealthStatus::Healthy,
            latency_ms: Some(42),
            ..BackendHealth::default()
        },
    );
    assert_eq!(reg.health("gpt4").unwrap().latency_ms, Some(42));
}

#[test]
fn update_health_records_last_check() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    let now = Utc::now();
    reg.update_health(
        "gpt4",
        BackendHealth {
            status: HealthStatus::Healthy,
            last_check: Some(now),
            ..BackendHealth::default()
        },
    );
    assert!(reg.health("gpt4").unwrap().last_check.is_some());
}

// ── 4. Filtering by health ────────────────────────────────────────────

#[test]
fn healthy_backends_includes_only_healthy() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.register_with_metadata("b", sample_metadata("b", "openai"));
    reg.update_health(
        "a",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    reg.update_health(
        "b",
        BackendHealth {
            status: HealthStatus::Unhealthy,
            ..BackendHealth::default()
        },
    );
    let healthy = reg.healthy_backends();
    assert_eq!(healthy, vec!["a"]);
}

#[test]
fn healthy_backends_excludes_degraded() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.update_health(
        "a",
        BackendHealth {
            status: HealthStatus::Degraded,
            ..BackendHealth::default()
        },
    );
    assert!(reg.healthy_backends().is_empty());
}

#[test]
fn healthy_backends_excludes_unknown() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    // default health is Unknown
    assert!(reg.healthy_backends().is_empty());
}

// ── 5. Filtering by dialect ───────────────────────────────────────────

#[test]
fn by_dialect_returns_matching() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    reg.register_with_metadata("claude", sample_metadata("claude", "anthropic"));
    reg.register_with_metadata("gpt3", sample_metadata("gpt3", "openai"));
    let openai = reg.by_dialect("openai");
    assert_eq!(openai, vec!["gpt3", "gpt4"]);
}

#[test]
fn by_dialect_returns_empty_for_unknown_dialect() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    assert!(reg.by_dialect("cohere").is_empty());
}

// ── 6. Rate limit serialization ───────────────────────────────────────

#[test]
fn rate_limit_serde_roundtrip() {
    let rl = RateLimit {
        requests_per_minute: 60,
        tokens_per_minute: 100_000,
        concurrent_requests: 5,
    };
    let json = serde_json::to_string(&rl).unwrap();
    let deser: RateLimit = serde_json::from_str(&json).unwrap();
    assert_eq!(deser, rl);
}

#[test]
fn metadata_with_rate_limit_serde_roundtrip() {
    let m = BackendMetadata {
        name: "gpt4".to_string(),
        dialect: "openai".to_string(),
        version: "1.0.0".to_string(),
        max_tokens: Some(8192),
        supports_streaming: true,
        supports_tools: false,
        rate_limit: Some(RateLimit {
            requests_per_minute: 120,
            tokens_per_minute: 200_000,
            concurrent_requests: 10,
        }),
    };
    let json = serde_json::to_string(&m).unwrap();
    let deser: BackendMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.name, "gpt4");
    assert_eq!(deser.rate_limit.as_ref().unwrap().requests_per_minute, 120);
}

#[test]
fn health_status_serde_roundtrip() {
    for status in [
        HealthStatus::Healthy,
        HealthStatus::Degraded,
        HealthStatus::Unhealthy,
        HealthStatus::Unknown,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let deser: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deser, status);
    }
}

#[test]
fn backend_health_serde_roundtrip() {
    let h = BackendHealth {
        status: HealthStatus::Degraded,
        last_check: Some(Utc::now()),
        latency_ms: Some(150),
        error_rate: 0.05,
        consecutive_failures: 1,
    };
    let json = serde_json::to_string(&h).unwrap();
    let deser: BackendHealth = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.status, HealthStatus::Degraded);
    assert_eq!(deser.latency_ms, Some(150));
}

// ── 7. Removal ─────────────────────────────────────────────────────────

#[test]
fn remove_returns_metadata() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    let removed = reg.remove("gpt4");
    assert!(removed.is_some());
    assert!(!reg.contains("gpt4"));
    assert!(reg.health("gpt4").is_none());
}

#[test]
fn remove_nonexistent_returns_none() {
    let mut reg = BackendRegistry::new();
    assert!(reg.remove("nope").is_none());
}

// ── 8. Re-registration / overwrite ─────────────────────────────────────

#[test]
fn re_register_overwrites_metadata() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    reg.register_with_metadata(
        "gpt4",
        BackendMetadata {
            name: "gpt4-turbo".to_string(),
            dialect: "openai".to_string(),
            version: "2.0.0".to_string(),
            max_tokens: Some(128_000),
            supports_streaming: true,
            supports_tools: true,
            rate_limit: None,
        },
    );
    assert_eq!(reg.metadata("gpt4").unwrap().version, "2.0.0");
    assert_eq!(reg.len(), 1);
}

#[test]
fn re_register_preserves_existing_health() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    reg.update_health(
        "gpt4",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    // Re-register should not reset health
    reg.register_with_metadata("gpt4", sample_metadata("gpt4-v2", "openai"));
    assert_eq!(reg.health("gpt4").unwrap().status, HealthStatus::Healthy);
}

// ── 9. Update health without registration ──────────────────────────────

#[test]
fn update_health_for_unregistered_backend() {
    let mut reg = BackendRegistry::new();
    reg.update_health(
        "ghost",
        BackendHealth {
            status: HealthStatus::Healthy,
            ..BackendHealth::default()
        },
    );
    // Health is stored even without metadata
    assert_eq!(reg.health("ghost").unwrap().status, HealthStatus::Healthy);
    // But it's not in the metadata registry
    assert!(!reg.contains("ghost"));
}
