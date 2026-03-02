//! Validates crates.io publication readiness for all workspace crates.
//!
//! Ensures every publishable crate has the required Cargo.toml metadata
//! (description, license, repository, readme, keywords, categories) and
//! that inter-workspace path dependencies carry an explicit version.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Metadata fields required for crates.io publication.
const REQUIRED_FIELDS: &[&str] = &[
    "description",
    "license",
    "repository",
    "readme",
    "keywords",
    "categories",
];

/// Workspace crates that are expected to be publishable.
const PUBLISHABLE_CRATES: &[&str] = &[
    "abp-backend-core",
    "abp-backend-mock",
    "abp-backend-sidecar",
    "abp-capability",
    "abp-claude-sdk",
    "abp-cli",
    "abp-codex-sdk",
    "abp-config",
    "abp-copilot-sdk",
    "abp-core",
    "abp-daemon",
    "abp-dialect",
    "abp-emulation",
    "abp-error",
    "abp-gemini-sdk",
    "abp-git",
    "abp-glob",
    "abp-host",
    "abp-integrations",
    "abp-kimi-sdk",
    "abp-mapping",
    "abp-openai-sdk",
    "abp-policy",
    "abp-projection",
    "abp-protocol",
    "abp-receipt",
    "abp-runtime",
    "abp-shim-claude",
    "abp-shim-codex",
    "abp-shim-copilot",
    "abp-shim-gemini",
    "abp-shim-kimi",
    "abp-shim-openai",
    "abp-sidecar-proto",
    "abp-sidecar-sdk",
    "abp-stream",
    "abp-telemetry",
    "abp-workspace",
    "claude-bridge",
    "sidecar-kit",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn crate_dir(name: &str) -> PathBuf {
    workspace_root().join("crates").join(name)
}

/// Parse a Cargo.toml and return raw content plus parsed toml table.
fn read_cargo_toml(name: &str) -> (String, toml::Table) {
    let path = crate_dir(name).join("Cargo.toml");
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: read error: {e}", name));
    let table: toml::Table = content
        .parse()
        .unwrap_or_else(|e| panic!("{}: parse error: {e}", name));
    (content, table)
}

fn package_table(table: &toml::Table) -> &toml::Table {
    table["package"]
        .as_table()
        .expect("[package] section missing")
}

/// Check if a field exists directly or via `.workspace = true`.
fn has_field(pkg: &toml::Table, field: &str) -> bool {
    if let Some(val) = pkg.get(field) {
        if val.is_table() {
            // e.g. `license.workspace = true`
            val.as_table()
                .map(|t| t.get("workspace").is_some())
                .unwrap_or(false)
        } else {
            true
        }
    } else {
        false
    }
}

#[test]
fn all_publishable_crates_have_cargo_toml() {
    for name in PUBLISHABLE_CRATES {
        let path = crate_dir(name).join("Cargo.toml");
        assert!(
            path.exists(),
            "{name}: Cargo.toml not found at {}",
            path.display()
        );
    }
}

#[test]
fn all_publishable_crates_have_required_metadata() {
    let mut failures: BTreeMap<&str, Vec<&str>> = BTreeMap::new();

    for name in PUBLISHABLE_CRATES {
        let (_content, table) = read_cargo_toml(name);
        let pkg = package_table(&table);

        let mut missing = Vec::new();
        for &field in REQUIRED_FIELDS {
            if !has_field(pkg, field) {
                missing.push(field);
            }
        }
        if !missing.is_empty() {
            failures.insert(name, missing);
        }
    }

    assert!(
        failures.is_empty(),
        "Crates missing required metadata:\n{}",
        failures
            .iter()
            .map(|(k, v)| format!("  {k}: {}", v.join(", ")))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn all_publishable_crates_have_readme() {
    for name in PUBLISHABLE_CRATES {
        let readme = crate_dir(name).join("README.md");
        assert!(readme.exists(), "{name}: README.md not found");
    }
}

#[test]
fn all_publishable_crates_have_license_files_at_root() {
    let root = workspace_root();
    assert!(
        root.join("LICENSE-MIT").exists(),
        "LICENSE-MIT missing at workspace root"
    );
    assert!(
        root.join("LICENSE-APACHE").exists(),
        "LICENSE-APACHE missing at workspace root"
    );
}

#[test]
fn workspace_version_is_consistent() {
    let root_toml: toml::Table = std::fs::read_to_string(workspace_root().join("Cargo.toml"))
        .expect("root Cargo.toml")
        .parse()
        .expect("parse root Cargo.toml");

    let ws_version = root_toml["workspace"]["package"]["version"]
        .as_str()
        .expect("workspace.package.version");

    for name in PUBLISHABLE_CRATES {
        let (_content, table) = read_cargo_toml(name);
        let pkg = package_table(&table);

        // Either version = "x.y.z" or version.workspace = true
        if let Some(ver) = pkg.get("version")
            && let Some(s) = ver.as_str()
        {
            assert_eq!(
                s, ws_version,
                "{name}: version {s} != workspace version {ws_version}"
            );
        }
    }
}

#[test]
fn path_dependencies_have_version() {
    let mut failures: Vec<String> = Vec::new();

    for name in PUBLISHABLE_CRATES {
        let (_content, table) = read_cargo_toml(name);

        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            let deps = match table.get(section).and_then(|v| v.as_table()) {
                Some(t) => t,
                None => continue,
            };

            for (dep_name, dep_val) in deps {
                let dep_table = match dep_val.as_table() {
                    Some(t) => t,
                    None => continue,
                };

                if dep_table.contains_key("path") && !dep_table.contains_key("version") {
                    failures.push(format!(
                        "  {name} -> {dep_name} ({section}): path dep without version"
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "Path dependencies missing version field:\n{}",
        failures.join("\n")
    );
}

#[test]
fn no_publish_false_on_publishable_crates() {
    for name in PUBLISHABLE_CRATES {
        let (_content, table) = read_cargo_toml(name);
        let pkg = package_table(&table);

        if let Some(publish) = pkg.get("publish")
            && let Some(b) = publish.as_bool()
        {
            assert!(
                b,
                "{name}: has publish = false but is in PUBLISHABLE_CRATES list"
            );
        }
    }
}

#[test]
fn keywords_within_crates_io_limits() {
    for name in PUBLISHABLE_CRATES {
        let (_content, table) = read_cargo_toml(name);
        let pkg = package_table(&table);

        if let Some(kw) = pkg.get("keywords").and_then(|v| v.as_array()) {
            assert!(
                kw.len() <= 5,
                "{name}: has {} keywords, crates.io allows max 5",
                kw.len()
            );
            for k in kw {
                let s = k.as_str().unwrap_or("");
                assert!(s.len() <= 20, "{name}: keyword '{s}' exceeds 20 char limit");
            }
        }
    }
}
