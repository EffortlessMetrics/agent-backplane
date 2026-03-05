#![allow(clippy::all)]
//! Tests for the enhanced backend core: metrics, selection, lifecycle, health helpers.

use abp_backend_core::health::{BackendHealth, HealthStatus};
use abp_backend_core::metadata::BackendMetadata;
use abp_backend_core::metrics::BackendMetrics;
use abp_backend_core::registry::BackendRegistry;
use abp_backend_core::selection::{SelectionStrategy, select_backend};

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

fn meta_no_stream(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        supports_streaming: false,
        ..sample_metadata(name, dialect)
    }
}

fn meta_no_tools(name: &str, dialect: &str) -> BackendMetadata {
    BackendMetadata {
        supports_tools: false,
        ..sample_metadata(name, dialect)
    }
}

fn make_healthy(reg: &mut BackendRegistry, name: &str) {
    reg.update_health(
        name,
        BackendHealth {
            status: HealthStatus::Healthy,
            latency_ms: Some(50),
            ..BackendHealth::default()
        },
    );
}

// ═══════════════════════════════════════════════════════════════════════
// BackendMetrics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn metrics_default_is_zeroed() {
    let m = BackendMetrics::default();
    assert_eq!(m.total_runs, 0);
    assert_eq!(m.successful_runs, 0);
    assert_eq!(m.failed_runs, 0);
    assert_eq!(m.total_latency_ms, 0);
    assert!(m.last_run_at.is_none());
}

#[test]
fn metrics_record_success() {
    let mut m = BackendMetrics::default();
    m.record_success(100);
    assert_eq!(m.total_runs, 1);
    assert_eq!(m.successful_runs, 1);
    assert_eq!(m.failed_runs, 0);
    assert_eq!(m.total_latency_ms, 100);
    assert!(m.last_run_at.is_some());
}

#[test]
fn metrics_record_failure() {
    let mut m = BackendMetrics::default();
    m.record_failure(200);
    assert_eq!(m.total_runs, 1);
    assert_eq!(m.successful_runs, 0);
    assert_eq!(m.failed_runs, 1);
    assert_eq!(m.total_latency_ms, 200);
    assert!(m.last_run_at.is_some());
}

#[test]
fn metrics_average_latency_none_when_empty() {
    let m = BackendMetrics::default();
    assert!(m.average_latency_ms().is_none());
}

#[test]
fn metrics_average_latency_computed() {
    let mut m = BackendMetrics::default();
    m.record_success(100);
    m.record_success(200);
    let avg = m.average_latency_ms().unwrap();
    assert!((avg - 150.0).abs() < f64::EPSILON);
}

#[test]
fn metrics_success_rate_none_when_empty() {
    let m = BackendMetrics::default();
    assert!(m.success_rate().is_none());
}

#[test]
fn metrics_success_rate_computed() {
    let mut m = BackendMetrics::default();
    m.record_success(10);
    m.record_success(10);
    m.record_failure(10);
    let rate = m.success_rate().unwrap();
    assert!((rate - 2.0 / 3.0).abs() < 1e-10);
}

#[test]
fn metrics_success_rate_all_failures() {
    let mut m = BackendMetrics::default();
    m.record_failure(10);
    m.record_failure(10);
    let rate = m.success_rate().unwrap();
    assert!((rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn metrics_serde_roundtrip() {
    let mut m = BackendMetrics::default();
    m.record_success(50);
    m.record_failure(100);
    let json = serde_json::to_string(&m).unwrap();
    let deser: BackendMetrics = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.total_runs, 2);
    assert_eq!(deser.successful_runs, 1);
    assert_eq!(deser.failed_runs, 1);
    assert_eq!(deser.total_latency_ms, 150);
}

// ═══════════════════════════════════════════════════════════════════════
// BackendHealth convenience methods
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn health_record_success_sets_healthy() {
    let mut h = BackendHealth::default();
    h.record_success(42);
    assert_eq!(h.status, HealthStatus::Healthy);
    assert_eq!(h.consecutive_failures, 0);
    assert_eq!(h.latency_ms, Some(42));
    assert!(h.last_check.is_some());
}

#[test]
fn health_record_failure_first_is_degraded() {
    let mut h = BackendHealth::default();
    h.record_failure(3);
    assert_eq!(h.status, HealthStatus::Degraded);
    assert_eq!(h.consecutive_failures, 1);
}

#[test]
fn health_record_failure_reaches_unhealthy() {
    let mut h = BackendHealth::default();
    h.record_failure(3);
    h.record_failure(3);
    h.record_failure(3);
    assert_eq!(h.status, HealthStatus::Unhealthy);
    assert_eq!(h.consecutive_failures, 3);
    assert!((h.error_rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn health_record_success_resets_failures() {
    let mut h = BackendHealth::default();
    h.record_failure(5);
    h.record_failure(5);
    h.record_success(10);
    assert_eq!(h.status, HealthStatus::Healthy);
    assert_eq!(h.consecutive_failures, 0);
}

#[test]
fn health_is_operational_healthy() {
    let h = BackendHealth {
        status: HealthStatus::Healthy,
        ..BackendHealth::default()
    };
    assert!(h.is_operational());
}

#[test]
fn health_is_operational_degraded() {
    let h = BackendHealth {
        status: HealthStatus::Degraded,
        ..BackendHealth::default()
    };
    assert!(h.is_operational());
}

#[test]
fn health_is_not_operational_unhealthy() {
    let h = BackendHealth {
        status: HealthStatus::Unhealthy,
        ..BackendHealth::default()
    };
    assert!(!h.is_operational());
}

#[test]
fn health_is_not_operational_unknown() {
    let h = BackendHealth::default();
    assert!(!h.is_operational());
}

// ═══════════════════════════════════════════════════════════════════════
// Registry metrics integration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_metrics_default_absent() {
    let reg = BackendRegistry::new();
    assert!(reg.metrics("nope").is_none());
}

#[test]
fn registry_metrics_mut_creates_default() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    let m = reg.metrics_mut("a");
    assert_eq!(m.total_runs, 0);
}

#[test]
fn registry_metrics_mut_accumulates() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.metrics_mut("a").record_success(100);
    reg.metrics_mut("a").record_failure(50);
    let m = reg.metrics("a").unwrap();
    assert_eq!(m.total_runs, 2);
    assert_eq!(m.total_latency_ms, 150);
}

#[test]
fn registry_remove_clears_metrics() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.metrics_mut("a").record_success(10);
    reg.remove("a");
    assert!(reg.metrics("a").is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// Registry capability filtering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn by_streaming_support_filters_correctly() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("s1", sample_metadata("s1", "openai"));
    reg.register_with_metadata("s2", meta_no_stream("s2", "openai"));
    assert_eq!(reg.by_streaming_support(), vec!["s1"]);
}

#[test]
fn by_tool_support_filters_correctly() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("t1", sample_metadata("t1", "openai"));
    reg.register_with_metadata("t2", meta_no_tools("t2", "openai"));
    assert_eq!(reg.by_tool_support(), vec!["t1"]);
}

#[test]
fn operational_backends_includes_healthy_and_degraded() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.register_with_metadata("b", sample_metadata("b", "openai"));
    reg.register_with_metadata("c", sample_metadata("c", "openai"));
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
            status: HealthStatus::Degraded,
            ..BackendHealth::default()
        },
    );
    reg.update_health(
        "c",
        BackendHealth {
            status: HealthStatus::Unhealthy,
            ..BackendHealth::default()
        },
    );
    assert_eq!(reg.operational_backends(), vec!["a", "b"]);
}

// ═══════════════════════════════════════════════════════════════════════
// Selection strategies
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn select_first_healthy_returns_first_alphabetical() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("beta", sample_metadata("beta", "openai"));
    reg.register_with_metadata("alpha", sample_metadata("alpha", "openai"));
    make_healthy(&mut reg, "alpha");
    make_healthy(&mut reg, "beta");
    let result = select_backend(&reg, &SelectionStrategy::FirstHealthy);
    assert_eq!(result.as_deref(), Some("alpha"));
}

#[test]
fn select_first_healthy_returns_none_when_empty() {
    let reg = BackendRegistry::new();
    assert!(select_backend(&reg, &SelectionStrategy::FirstHealthy).is_none());
}

#[test]
fn select_first_healthy_skips_unhealthy() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    reg.update_health(
        "a",
        BackendHealth {
            status: HealthStatus::Unhealthy,
            ..BackendHealth::default()
        },
    );
    assert!(select_backend(&reg, &SelectionStrategy::FirstHealthy).is_none());
}

#[test]
fn select_by_dialect_returns_healthy_match() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    reg.register_with_metadata("claude", sample_metadata("claude", "anthropic"));
    make_healthy(&mut reg, "gpt4");
    make_healthy(&mut reg, "claude");
    let result = select_backend(&reg, &SelectionStrategy::ByDialect("anthropic".into()));
    assert_eq!(result.as_deref(), Some("claude"));
}

#[test]
fn select_by_dialect_skips_unhealthy() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("gpt4", sample_metadata("gpt4", "openai"));
    reg.update_health(
        "gpt4",
        BackendHealth {
            status: HealthStatus::Unhealthy,
            ..BackendHealth::default()
        },
    );
    assert!(select_backend(&reg, &SelectionStrategy::ByDialect("openai".into())).is_none());
}

#[test]
fn select_by_preference_returns_named_if_healthy() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("my-fav", sample_metadata("my-fav", "openai"));
    make_healthy(&mut reg, "my-fav");
    let result = select_backend(&reg, &SelectionStrategy::ByPreference("my-fav".into()));
    assert_eq!(result.as_deref(), Some("my-fav"));
}

#[test]
fn select_by_preference_returns_none_if_unhealthy() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("my-fav", sample_metadata("my-fav", "openai"));
    assert!(select_backend(&reg, &SelectionStrategy::ByPreference("my-fav".into())).is_none());
}

#[test]
fn select_by_preference_returns_none_if_missing() {
    let reg = BackendRegistry::new();
    assert!(select_backend(&reg, &SelectionStrategy::ByPreference("ghost".into())).is_none());
}

#[test]
fn select_by_streaming_picks_streaming_backend() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("s", sample_metadata("s", "openai"));
    reg.register_with_metadata("ns", meta_no_stream("ns", "openai"));
    make_healthy(&mut reg, "s");
    make_healthy(&mut reg, "ns");
    let result = select_backend(&reg, &SelectionStrategy::ByStreaming);
    assert_eq!(result.as_deref(), Some("s"));
}

#[test]
fn select_by_tool_support_picks_tools_backend() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("t", sample_metadata("t", "openai"));
    reg.register_with_metadata("nt", meta_no_tools("nt", "openai"));
    make_healthy(&mut reg, "t");
    make_healthy(&mut reg, "nt");
    let result = select_backend(&reg, &SelectionStrategy::ByToolSupport);
    assert_eq!(result.as_deref(), Some("t"));
}

#[test]
fn select_by_lowest_latency_picks_fastest() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("slow", sample_metadata("slow", "openai"));
    reg.register_with_metadata("fast", sample_metadata("fast", "openai"));
    reg.update_health(
        "slow",
        BackendHealth {
            status: HealthStatus::Healthy,
            latency_ms: Some(500),
            ..BackendHealth::default()
        },
    );
    reg.update_health(
        "fast",
        BackendHealth {
            status: HealthStatus::Healthy,
            latency_ms: Some(10),
            ..BackendHealth::default()
        },
    );
    let result = select_backend(&reg, &SelectionStrategy::ByLowestLatency);
    assert_eq!(result.as_deref(), Some("fast"));
}

#[test]
fn select_by_lowest_latency_skips_unhealthy() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("fast", sample_metadata("fast", "openai"));
    reg.register_with_metadata("slow", sample_metadata("slow", "openai"));
    reg.update_health(
        "fast",
        BackendHealth {
            status: HealthStatus::Unhealthy,
            latency_ms: Some(1),
            ..BackendHealth::default()
        },
    );
    reg.update_health(
        "slow",
        BackendHealth {
            status: HealthStatus::Healthy,
            latency_ms: Some(500),
            ..BackendHealth::default()
        },
    );
    let result = select_backend(&reg, &SelectionStrategy::ByLowestLatency);
    assert_eq!(result.as_deref(), Some("slow"));
}

#[test]
fn select_by_lowest_latency_returns_none_when_no_latency() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    make_healthy(&mut reg, "a");
    // make_healthy sets latency_ms, so override with None
    reg.update_health(
        "a",
        BackendHealth {
            status: HealthStatus::Healthy,
            latency_ms: None,
            ..BackendHealth::default()
        },
    );
    assert!(select_backend(&reg, &SelectionStrategy::ByLowestLatency).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// Registry select() convenience
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_select_delegates_to_strategy() {
    let mut reg = BackendRegistry::new();
    reg.register_with_metadata("a", sample_metadata("a", "openai"));
    make_healthy(&mut reg, "a");
    let result = reg.select(&SelectionStrategy::FirstHealthy);
    assert_eq!(result.as_deref(), Some("a"));
}

// ═══════════════════════════════════════════════════════════════════════
// SelectionStrategy equality / debug
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn selection_strategy_equality() {
    assert_eq!(
        SelectionStrategy::FirstHealthy,
        SelectionStrategy::FirstHealthy
    );
    assert_ne!(
        SelectionStrategy::ByDialect("a".into()),
        SelectionStrategy::ByDialect("b".into())
    );
}

#[test]
fn selection_strategy_debug_format() {
    let s = format!("{:?}", SelectionStrategy::ByStreaming);
    assert!(s.contains("ByStreaming"));
}
