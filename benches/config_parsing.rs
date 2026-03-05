// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmarks for config TOML parsing, validation, serialization, and merging.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::collections::BTreeMap;

use abp_config::validate::ConfigValidator;
use abp_config::{
    BackendEntry, BackplaneConfig, merge_configs, parse_toml, to_toml, to_toml_pretty,
    validate_config,
};

// ═══════════════════════════════════════════════════════════════════════════
// TOML fixtures — small, medium, large
// ═══════════════════════════════════════════════════════════════════════════

fn small_toml() -> &'static str {
    r#"
default_backend = "mock"
log_level = "info"

[backends.mock]
type = "mock"
"#
}

fn medium_toml() -> String {
    let mut s = String::from(
        r#"
default_backend = "openai"
workspace_dir = "/tmp/abp-workspace"
log_level = "debug"
receipts_dir = "./data/receipts"
bind_address = "127.0.0.1"
port = 8080

[backends.mock]
type = "mock"

[backends.openai]
type = "sidecar"
command = "node"
args = ["hosts/node/index.js"]
timeout_secs = 300

[backends.anthropic]
type = "sidecar"
command = "python3"
args = ["hosts/python/main.py"]
timeout_secs = 600
"#,
    );
    // Add a few policy profiles
    s.push_str("policy_profiles = [\"default\", \"strict\", \"permissive\"]\n");
    s
}

fn large_toml() -> String {
    let mut s = String::from(
        r#"
default_backend = "openai"
workspace_dir = "/var/lib/abp/workspaces"
log_level = "trace"
receipts_dir = "/var/lib/abp/receipts"
bind_address = "0.0.0.0"
port = 9090
"#,
    );
    // Generate many backends
    for i in 0..50 {
        s.push_str(&format!(
            r#"
[backends.sidecar_{i}]
type = "sidecar"
command = "node"
args = ["hosts/node/sidecar_{i}.js", "--port", "{port}"]
timeout_secs = {timeout}
"#,
            i = i,
            port = 3000 + i,
            timeout = 60 + i * 10,
        ));
    }
    s.push_str("policy_profiles = [");
    for i in 0..20 {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!("\"profile_{i}\""));
    }
    s.push_str("]\n");
    s
}

// ═══════════════════════════════════════════════════════════════════════════
// Config structs for non-TOML benchmarks
// ═══════════════════════════════════════════════════════════════════════════

fn small_config() -> BackplaneConfig {
    BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("info".into()),
        backends: {
            let mut m = BTreeMap::new();
            m.insert("mock".into(), BackendEntry::Mock {});
            m
        },
        ..BackplaneConfig::default()
    }
}

fn medium_config() -> BackplaneConfig {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    backends.insert(
        "openai".into(),
        BackendEntry::Sidecar {
            command: "node".into(),
            args: vec!["hosts/node/index.js".into()],
            timeout_secs: Some(300),
        },
    );
    backends.insert(
        "anthropic".into(),
        BackendEntry::Sidecar {
            command: "python3".into(),
            args: vec!["hosts/python/main.py".into()],
            timeout_secs: Some(600),
        },
    );
    BackplaneConfig {
        default_backend: Some("openai".into()),
        workspace_dir: Some("/tmp/abp-workspace".into()),
        log_level: Some("debug".into()),
        receipts_dir: Some("./data/receipts".into()),
        bind_address: Some("127.0.0.1".into()),
        port: Some(8080),
        policy_profiles: vec!["default".into(), "strict".into()],
        backends,
    }
}

fn large_config() -> BackplaneConfig {
    let mut backends = BTreeMap::new();
    backends.insert("mock".into(), BackendEntry::Mock {});
    for i in 0..50 {
        backends.insert(
            format!("sidecar_{i}"),
            BackendEntry::Sidecar {
                command: "node".into(),
                args: vec![format!("hosts/node/sidecar_{i}.js")],
                timeout_secs: Some(60 + i * 10),
            },
        );
    }
    BackplaneConfig {
        default_backend: Some("sidecar_0".into()),
        workspace_dir: Some("/var/lib/abp/workspaces".into()),
        log_level: Some("trace".into()),
        receipts_dir: Some("/var/lib/abp/receipts".into()),
        bind_address: Some("0.0.0.0".into()),
        port: Some(9090),
        policy_profiles: (0..20).map(|i| format!("profile_{i}")).collect(),
        backends,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. TOML parsing (parse_toml) — the primary config hot path
// ═══════════════════════════════════════════════════════════════════════════

fn bench_parse_toml(c: &mut Criterion) {
    let mut group = c.benchmark_group("config/parse_toml");

    let small = small_toml().to_string();
    let med = medium_toml();
    let large = large_toml();

    for (label, toml_str) in [("small", &small), ("medium", &med), ("large", &large)] {
        group.throughput(Throughput::Bytes(toml_str.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), toml_str, |b, input| {
            b.iter(|| parse_toml(black_box(input)).unwrap());
        });
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Config validation
// ═══════════════════════════════════════════════════════════════════════════

fn bench_validate_config(c: &mut Criterion) {
    let mut group = c.benchmark_group("config/validate");

    for (label, config) in [
        ("small", small_config()),
        ("medium", medium_config()),
        ("large", large_config()),
    ] {
        group.bench_with_input(
            BenchmarkId::new("validate_config", label),
            &config,
            |b, cfg| {
                b.iter(|| validate_config(black_box(cfg)));
            },
        );
    }

    // Structured validator
    for (label, config) in [
        ("small", small_config()),
        ("medium", medium_config()),
        ("large", large_config()),
    ] {
        group.bench_with_input(
            BenchmarkId::new("config_validator", label),
            &config,
            |b, cfg| {
                b.iter(|| ConfigValidator::validate(black_box(cfg)));
            },
        );
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Config serialization — to_toml / to_toml_pretty / JSON
// ═══════════════════════════════════════════════════════════════════════════

fn bench_config_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("config/serialize");

    for (label, config) in [
        ("small", small_config()),
        ("medium", medium_config()),
        ("large", large_config()),
    ] {
        group.bench_with_input(BenchmarkId::new("to_toml", label), &config, |b, cfg| {
            b.iter(|| to_toml(black_box(cfg)).unwrap());
        });

        group.bench_with_input(
            BenchmarkId::new("to_toml_pretty", label),
            &config,
            |b, cfg| {
                b.iter(|| to_toml_pretty(black_box(cfg)).unwrap());
            },
        );

        group.bench_with_input(BenchmarkId::new("to_json", label), &config, |b, cfg| {
            b.iter(|| serde_json::to_string(black_box(cfg)).unwrap());
        });
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Config merging
// ═══════════════════════════════════════════════════════════════════════════

fn bench_config_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("config/merge");

    let base = small_config();
    let overlay = medium_config();
    group.bench_function("small_over_medium", |b| {
        b.iter(|| merge_configs(black_box(base.clone()), black_box(overlay.clone())));
    });

    let large_base = large_config();
    let large_overlay = large_config();
    group.bench_function("large_over_large", |b| {
        b.iter(|| {
            merge_configs(
                black_box(large_base.clone()),
                black_box(large_overlay.clone()),
            )
        });
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. TOML roundtrip — parse → serialize → parse
// ═══════════════════════════════════════════════════════════════════════════

fn bench_config_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("config/roundtrip");

    for (label, config) in [
        ("small", small_config()),
        ("medium", medium_config()),
        ("large", large_config()),
    ] {
        group.bench_with_input(
            BenchmarkId::new("toml_roundtrip", label),
            &config,
            |b, cfg| {
                b.iter(|| {
                    let toml_str = to_toml(black_box(cfg)).unwrap();
                    parse_toml(&toml_str).unwrap()
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("json_roundtrip", label),
            &config,
            |b, cfg| {
                b.iter(|| {
                    let json = serde_json::to_string(black_box(cfg)).unwrap();
                    serde_json::from_str::<BackplaneConfig>(&json).unwrap()
                });
            },
        );
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// Criterion groups & main
// ═══════════════════════════════════════════════════════════════════════════

criterion_group!(
    benches,
    bench_parse_toml,
    bench_validate_config,
    bench_config_serialize,
    bench_config_merge,
    bench_config_roundtrip,
);
criterion_main!(benches);
