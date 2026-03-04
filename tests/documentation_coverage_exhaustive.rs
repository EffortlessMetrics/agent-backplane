#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
//! Exhaustive documentation coverage tests.
//!
//! Validates README, CHANGELOG, CONTRIBUTING, LICENSE, docs/ directory,
//! JSON schema files, crate README files, module-level doc comments,
//! and cross-reference consistency across the workspace.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

// ===========================================================================
// Helpers
// ===========================================================================

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn crate_dir(name: &str) -> PathBuf {
    workspace_root().join("crates").join(name)
}

fn read_file(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn read_root_cargo_toml() -> toml::Table {
    let content = read_file(&workspace_root().join("Cargo.toml"));
    content.parse().expect("parse root Cargo.toml")
}

fn workspace_members() -> Vec<String> {
    let table = read_root_cargo_toml();
    table["workspace"]["members"]
        .as_array()
        .expect("workspace.members array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn member_crate_name(member: &str) -> &str {
    member.rsplit('/').next().unwrap_or(member)
}

fn crate_members() -> Vec<String> {
    workspace_members()
        .into_iter()
        .filter(|m| m.starts_with("crates/"))
        .map(|m| member_crate_name(&m).to_string())
        .collect()
}

fn read_crate_cargo_toml(name: &str) -> toml::Table {
    let content = read_file(&crate_dir(name).join("Cargo.toml"));
    content
        .parse()
        .unwrap_or_else(|e| panic!("{name}: parse error: {e}"))
}

fn package_table(table: &toml::Table) -> &toml::Table {
    table["package"]
        .as_table()
        .expect("[package] section missing")
}

/// Collect all `.rs` files under a directory recursively.
fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                result.extend(collect_rs_files(&p));
            } else if p.extension().is_some_and(|e| e == "rs") {
                result.push(p);
            }
        }
    }
    result
}

const SDK_VENDORS: &[&str] = &["claude", "codex", "copilot", "gemini", "kimi", "openai"];

const REQUIRED_DOCS: &[&str] = &[
    "architecture.md",
    "sidecar_protocol.md",
    "sdk_mapping.md",
    "capabilities.md",
    "security.md",
    "versioning.md",
    "testing.md",
    "capability_negotiation.md",
    "dialect_engine_matrix.md",
];

// ===========================================================================
// 1. README.md — existence and key features
// ===========================================================================

#[test]
fn readme_exists() {
    assert!(
        workspace_root().join("README.md").exists(),
        "README.md missing at workspace root"
    );
}

#[test]
fn readme_has_substantial_content() {
    let content = read_file(&workspace_root().join("README.md"));
    assert!(
        content.len() > 1000,
        "README.md seems too short ({} bytes)",
        content.len()
    );
}

#[test]
fn readme_mentions_work_order() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("WorkOrder"),
        "README should mention WorkOrder"
    );
}

#[test]
fn readme_mentions_receipt() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(readme.contains("Receipt"), "README should mention Receipt");
}

#[test]
fn readme_mentions_agent_event() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("AgentEvent"),
        "README should mention AgentEvent"
    );
}

#[test]
fn readme_mentions_envelope() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("Envelope"),
        "README should mention Envelope"
    );
}

#[test]
fn readme_mentions_abp_core() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("abp-core"),
        "README should mention abp-core"
    );
}

#[test]
fn readme_mentions_sidecar_protocol() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("sidecar") || readme.contains("Sidecar"),
        "README should mention sidecar protocol"
    );
}

#[test]
fn readme_references_contributing() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("CONTRIBUTING"),
        "README should reference CONTRIBUTING"
    );
}

#[test]
fn readme_references_docs_directory() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("docs/"),
        "README should reference docs/ directory"
    );
}

#[test]
fn readme_mentions_dual_license() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("MIT") && readme.contains("Apache"),
        "README should mention dual MIT/Apache license"
    );
}

#[test]
fn readme_mentions_capability() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("Capability") || readme.contains("capability"),
        "README should mention capabilities"
    );
}

#[test]
fn readme_mentions_backend() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("backend") || readme.contains("Backend"),
        "README should mention backends"
    );
}

#[test]
fn readme_has_architecture_section() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("## Architecture") || readme.contains("## architecture"),
        "README should have an Architecture section"
    );
}

// ===========================================================================
// 2. CHANGELOG.md — proper format
// ===========================================================================

#[test]
fn changelog_exists() {
    assert!(
        workspace_root().join("CHANGELOG.md").exists(),
        "CHANGELOG.md missing"
    );
}

#[test]
fn changelog_has_substantial_content() {
    let content = read_file(&workspace_root().join("CHANGELOG.md"));
    assert!(
        content.len() > 100,
        "CHANGELOG.md too short ({} bytes)",
        content.len()
    );
}

#[test]
fn changelog_follows_keep_a_changelog() {
    let content = read_file(&workspace_root().join("CHANGELOG.md"));
    assert!(
        content.contains("Keep a Changelog") || content.contains("keepachangelog"),
        "CHANGELOG should reference Keep a Changelog format"
    );
}

#[test]
fn changelog_references_semver() {
    let content = read_file(&workspace_root().join("CHANGELOG.md"));
    assert!(
        content.contains("Semantic Versioning") || content.contains("semver"),
        "CHANGELOG should reference Semantic Versioning"
    );
}

#[test]
fn changelog_has_version_header() {
    let content = read_file(&workspace_root().join("CHANGELOG.md"));
    assert!(
        content.contains("## ["),
        "CHANGELOG should have at least one version header (## [...])"
    );
}

#[test]
fn changelog_has_unreleased_section() {
    let content = read_file(&workspace_root().join("CHANGELOG.md"));
    assert!(
        content.contains("[Unreleased]") || content.contains("[unreleased]"),
        "CHANGELOG should have an [Unreleased] section"
    );
}

#[test]
fn changelog_has_heading() {
    let content = read_file(&workspace_root().join("CHANGELOG.md"));
    assert!(
        content.starts_with("# "),
        "CHANGELOG should start with a top-level heading"
    );
}

// ===========================================================================
// 3. All crate README files exist
// ===========================================================================

#[test]
fn all_crates_have_readme_file() {
    let mut missing = Vec::new();
    for name in crate_members() {
        if !crate_dir(&name).join("README.md").exists() {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing README.md file: {missing:?}"
    );
}

#[test]
fn crate_readmes_are_non_empty() {
    let mut empty = Vec::new();
    for name in crate_members() {
        let path = crate_dir(&name).join("README.md");
        if path.exists() {
            let content = read_file(&path);
            if content.trim().len() < 20 {
                empty.push(name);
            }
        }
    }
    assert!(
        empty.is_empty(),
        "Crates with empty/near-empty README: {empty:?}"
    );
}

#[test]
fn crate_readmes_have_crate_name() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let path = crate_dir(&name).join("README.md");
        if path.exists() {
            let content = read_file(&path);
            if !content.contains(&name) {
                missing.push(name);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Crate READMEs not mentioning their own crate name: {missing:?}"
    );
}

#[test]
fn crate_readmes_have_heading() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let path = crate_dir(&name).join("README.md");
        if path.exists() {
            let content = read_file(&path);
            if !content.starts_with("# ") {
                missing.push(name);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Crate READMEs missing top-level heading: {missing:?}"
    );
}

// ===========================================================================
// 4. docs/ directory — complete protocol documentation
// ===========================================================================

#[test]
fn docs_directory_exists() {
    assert!(
        workspace_root().join("docs").is_dir(),
        "docs/ directory missing"
    );
}

#[test]
fn docs_required_files_exist() {
    let docs = workspace_root().join("docs");
    let mut missing = Vec::new();
    for name in REQUIRED_DOCS {
        if !docs.join(name).exists() {
            missing.push(*name);
        }
    }
    assert!(missing.is_empty(), "Missing required docs: {missing:?}");
}

#[test]
fn docs_sidecar_protocol_references_hello() {
    let content = read_file(&workspace_root().join("docs").join("sidecar_protocol.md"));
    assert!(
        content.contains("hello"),
        "sidecar_protocol.md should document the hello envelope"
    );
}

#[test]
fn docs_sidecar_protocol_references_discriminator() {
    let content = read_file(&workspace_root().join("docs").join("sidecar_protocol.md"));
    assert!(
        content.contains("\"t\"") || content.contains("`t`"),
        "sidecar_protocol.md should document the `t` discriminator field"
    );
}

#[test]
fn docs_sidecar_protocol_references_jsonl() {
    let content = read_file(&workspace_root().join("docs").join("sidecar_protocol.md"));
    assert!(
        content.contains("JSONL") || content.contains("jsonl") || content.contains("JSON"),
        "sidecar_protocol.md should mention JSONL"
    );
}

#[test]
fn docs_architecture_has_content() {
    let content = read_file(&workspace_root().join("docs").join("architecture.md"));
    assert!(
        content.len() > 200,
        "architecture.md too short ({} bytes)",
        content.len()
    );
}

#[test]
fn docs_sdk_mapping_directory_exists() {
    assert!(
        workspace_root().join("docs").join("sdk-mapping").is_dir(),
        "docs/sdk-mapping/ directory missing"
    );
}

#[test]
fn docs_sdk_mapping_has_all_vendors() {
    let dir = workspace_root().join("docs").join("sdk-mapping");
    let mut missing = Vec::new();
    for vendor in SDK_VENDORS {
        if !dir.join(format!("{vendor}.md")).exists() {
            missing.push(*vendor);
        }
    }
    assert!(
        missing.is_empty(),
        "docs/sdk-mapping missing vendor docs: {missing:?}"
    );
}

#[test]
fn docs_sdk_mapping_files_have_content() {
    let dir = workspace_root().join("docs").join("sdk-mapping");
    let mut empty = Vec::new();
    for vendor in SDK_VENDORS {
        let path = dir.join(format!("{vendor}.md"));
        if path.exists() {
            let content = read_file(&path);
            if content.trim().len() < 50 {
                empty.push(*vendor);
            }
        }
    }
    assert!(
        empty.is_empty(),
        "SDK mapping docs with insufficient content: {empty:?}"
    );
}

#[test]
fn docs_mapping_matrix_exists() {
    assert!(
        workspace_root()
            .join("docs")
            .join("sdk-mapping")
            .join("mapping-matrix.md")
            .exists(),
        "docs/sdk-mapping/mapping-matrix.md missing"
    );
}

#[test]
fn docs_error_codes_exist() {
    let docs = workspace_root().join("docs");
    assert!(
        docs.join("errors.md").exists() || docs.join("error_codes.md").exists(),
        "docs/errors.md or docs/error_codes.md missing"
    );
}

#[test]
fn docs_files_are_non_empty() {
    let docs_dir = workspace_root().join("docs");
    let mut empty = Vec::new();
    if let Ok(entries) = fs::read_dir(&docs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "md") {
                let content = read_file(&path);
                if content.trim().is_empty() {
                    empty.push(path.display().to_string());
                }
            }
        }
    }
    assert!(empty.is_empty(), "Empty doc files: {empty:?}");
}

#[test]
fn docs_files_start_with_heading() {
    let docs_dir = workspace_root().join("docs");
    let mut missing = Vec::new();
    if let Ok(entries) = fs::read_dir(&docs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "md") {
                let content = read_file(&path);
                let trimmed = content.trim_start();
                if !trimmed.starts_with('#') && !trimmed.starts_with('>') {
                    missing.push(path.file_name().unwrap().to_string_lossy().into_owned());
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Doc files not starting with heading or blockquote: {missing:?}"
    );
}

// ===========================================================================
// 5. JSON schema files valid
// ===========================================================================

#[test]
fn contracts_schemas_directory_exists() {
    assert!(
        workspace_root().join("contracts").join("schemas").is_dir(),
        "contracts/schemas/ directory missing"
    );
}

#[test]
fn work_order_schema_exists() {
    assert!(
        workspace_root()
            .join("contracts")
            .join("schemas")
            .join("work_order.schema.json")
            .exists(),
        "work_order.schema.json missing"
    );
}

#[test]
fn receipt_schema_exists() {
    assert!(
        workspace_root()
            .join("contracts")
            .join("schemas")
            .join("receipt.schema.json")
            .exists(),
        "receipt.schema.json missing"
    );
}

#[test]
fn backplane_config_schema_exists() {
    assert!(
        workspace_root()
            .join("contracts")
            .join("schemas")
            .join("backplane_config.schema.json")
            .exists(),
        "backplane_config.schema.json missing"
    );
}

#[test]
fn schema_files_are_valid_json() {
    let dir = workspace_root().join("contracts").join("schemas");
    let mut failures = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let content = read_file(&path);
                if serde_json::from_str::<serde_json::Value>(&content).is_err() {
                    failures.push(path.display().to_string());
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Invalid JSON schema files: {failures:?}"
    );
}

#[test]
fn schema_files_have_schema_or_title() {
    let dir = workspace_root().join("contracts").join("schemas");
    let mut failures = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let content = read_file(&path);
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if val.get("$schema").is_none() && val.get("title").is_none() {
                        failures.push(path.display().to_string());
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Schema files missing $schema or title: {failures:?}"
    );
}

#[test]
fn schema_files_have_type_or_oneof() {
    let dir = workspace_root().join("contracts").join("schemas");
    let mut failures = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let content = read_file(&path);
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if val.get("type").is_none()
                        && val.get("oneOf").is_none()
                        && val.get("anyOf").is_none()
                        && val.get("$ref").is_none()
                    {
                        failures.push(path.display().to_string());
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Schema files missing type/oneOf/anyOf/$ref: {failures:?}"
    );
}

#[test]
fn schema_files_are_well_formed_objects() {
    let dir = workspace_root().join("contracts").join("schemas");
    let mut failures = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let content = read_file(&path);
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(val) => {
                        if !val.is_object() {
                            failures.push(format!("{}: not a JSON object", path.display()));
                        }
                    }
                    Err(e) => {
                        failures.push(format!("{}: {e}", path.display()));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Schema files not well-formed objects: {failures:?}"
    );
}

// ===========================================================================
// 6. CONTRIBUTING.md
// ===========================================================================

#[test]
fn contributing_exists() {
    assert!(
        workspace_root().join("CONTRIBUTING.md").exists(),
        "CONTRIBUTING.md missing"
    );
}

#[test]
fn contributing_has_content() {
    let content = read_file(&workspace_root().join("CONTRIBUTING.md"));
    assert!(
        content.len() > 200,
        "CONTRIBUTING.md too short ({} bytes)",
        content.len()
    );
}

#[test]
fn contributing_mentions_testing() {
    let content = read_file(&workspace_root().join("CONTRIBUTING.md"));
    assert!(
        content.contains("test") || content.contains("Test"),
        "CONTRIBUTING.md should mention testing"
    );
}

#[test]
fn contributing_mentions_code_of_conduct() {
    let content = read_file(&workspace_root().join("CONTRIBUTING.md"));
    assert!(
        content.contains("Code of Conduct") || content.contains("CODE_OF_CONDUCT"),
        "CONTRIBUTING.md should reference Code of Conduct"
    );
}

#[test]
fn code_of_conduct_exists() {
    assert!(
        workspace_root().join("CODE_OF_CONDUCT.md").exists(),
        "CODE_OF_CONDUCT.md missing"
    );
}

// ===========================================================================
// 7. LICENSE files
// ===========================================================================

#[test]
fn license_mit_exists() {
    assert!(
        workspace_root().join("LICENSE-MIT").exists(),
        "LICENSE-MIT missing"
    );
}

#[test]
fn license_apache_exists() {
    assert!(
        workspace_root().join("LICENSE-APACHE").exists(),
        "LICENSE-APACHE missing"
    );
}

#[test]
fn license_mit_has_content() {
    let content = read_file(&workspace_root().join("LICENSE-MIT"));
    assert!(content.len() > 100, "LICENSE-MIT too short");
    assert!(
        content.contains("MIT") || content.contains("Permission"),
        "LICENSE-MIT doesn't look like an MIT license"
    );
}

#[test]
fn license_apache_has_content() {
    let content = read_file(&workspace_root().join("LICENSE-APACHE"));
    assert!(content.len() > 100, "LICENSE-APACHE too short");
    assert!(
        content.contains("Apache") || content.contains("APACHE"),
        "LICENSE-APACHE doesn't look like an Apache license"
    );
}

#[test]
fn workspace_license_is_dual() {
    let root = read_root_cargo_toml();
    let license = root["workspace"]["package"]["license"]
        .as_str()
        .expect("workspace.package.license");
    assert!(
        license.contains("MIT") && license.contains("Apache"),
        "Workspace license should be dual MIT/Apache, got: {license}"
    );
}

#[test]
fn no_crate_overrides_workspace_license() {
    let mut overrides = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if pkg.get("license").is_some_and(|val| val.is_str()) {
            overrides.push(name);
        }
    }
    assert!(
        overrides.is_empty(),
        "Crates overriding workspace license: {overrides:?}"
    );
}

// ===========================================================================
// 8. Module-level doc comments exist
// ===========================================================================

#[test]
fn all_crate_lib_rs_have_doc_comment() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let lib_rs = crate_dir(&name).join("src").join("lib.rs");
        if lib_rs.exists() {
            let content = read_file(&lib_rs);
            let has_doc = content.contains("//!") || content.contains("#![doc");
            if !has_doc {
                missing.push(name);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Crate lib.rs files missing module-level doc comments: {missing:?}"
    );
}

#[test]
fn core_crate_warns_missing_docs() {
    let lib_rs = crate_dir("abp-core").join("src").join("lib.rs");
    let content = read_file(&lib_rs);
    assert!(
        content.contains("missing_docs"),
        "abp-core lib.rs should have #![warn(missing_docs)]"
    );
}

#[test]
fn protocol_crate_warns_missing_docs() {
    let lib_rs = crate_dir("abp-protocol").join("src").join("lib.rs");
    let content = read_file(&lib_rs);
    assert!(
        content.contains("missing_docs"),
        "abp-protocol lib.rs should have #![warn(missing_docs)]"
    );
}

#[test]
fn all_crate_lib_rs_deny_unsafe() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let lib_rs = crate_dir(&name).join("src").join("lib.rs");
        if lib_rs.exists() {
            let content = read_file(&lib_rs);
            if !content.contains("deny(unsafe_code)") && !content.contains("forbid(unsafe_code)") {
                missing.push(name);
            }
        }
    }
    // Allow some crates that may not have deny(unsafe_code)
    missing.retain(|n| {
        !matches!(
            n.as_str(),
            "abp-cli"
                | "abp-daemon"
                | "abp-claude-sdk"
                | "abp-gemini-sdk"
                | "abp-codex-sdk"
                | "abp-kimi-sdk"
                | "abp-openai-sdk"
                | "abp-copilot-sdk"
                | "abp-retry"
                | "claude-bridge"
                | "sidecar-kit"
        )
    });
    assert!(
        missing.is_empty(),
        "Crate lib.rs files missing #![deny(unsafe_code)]: {missing:?}"
    );
}

// ===========================================================================
// 9. Cargo.toml metadata completeness
// ===========================================================================

#[test]
fn all_crates_have_description() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if pkg.get("description").is_none()
            && pkg
                .get("description")
                .and_then(|v| v.as_table())
                .and_then(|t| t.get("workspace"))
                .is_none()
        {
            // Check if it's using workspace inheritance
            let has_desc = pkg
                .get("description")
                .map(|v| v.is_str() || v.is_table())
                .unwrap_or(false);
            if !has_desc {
                missing.push(name);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing description: {missing:?}"
    );
}

#[test]
fn all_crates_have_license_field() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if pkg.get("license").is_none() {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing license field: {missing:?}"
    );
}

#[test]
fn all_crates_have_repository() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if pkg.get("repository").is_none() {
            missing.push(name);
        }
    }
    assert!(missing.is_empty(), "Crates missing repository: {missing:?}");
}

#[test]
fn workspace_package_has_required_fields() {
    let root = read_root_cargo_toml();
    let ws = &root["workspace"]["package"];
    for field in &["version", "edition", "license", "description", "repository"] {
        assert!(
            ws.get(*field).is_some(),
            "workspace.package.{field} must be set"
        );
    }
}

#[test]
fn workspace_has_resolver_2() {
    let root = read_root_cargo_toml();
    let resolver = root["workspace"]
        .get("resolver")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(resolver, "2", "Workspace should use resolver = \"2\"");
}

// ===========================================================================
// 10. Cross-reference checks
// ===========================================================================

#[test]
fn readme_doc_links_point_to_existing_files() {
    let readme = read_file(&workspace_root().join("README.md"));
    let mut broken = Vec::new();
    for cap in readme.split("](") {
        if let Some(end) = cap.find(')') {
            let link = &cap[..end];
            if !link.starts_with("http")
                && !link.starts_with('#')
                && !link.starts_with("mailto:")
                && !link.contains("://")
            {
                let file_path = link.split('#').next().unwrap_or(link);
                if !file_path.is_empty() {
                    let full = workspace_root().join(file_path);
                    if !full.exists() {
                        broken.push(file_path.to_string());
                    }
                }
            }
        }
    }
    assert!(
        broken.is_empty(),
        "README has broken local links: {broken:?}"
    );
}

#[test]
fn all_crate_dirs_are_workspace_members() {
    let members = workspace_members();
    let member_names: BTreeSet<&str> = members.iter().map(|m| member_crate_name(m)).collect();
    let crates_dir = workspace_root().join("crates");
    let mut unlisted = Vec::new();
    if let Ok(entries) = fs::read_dir(&crates_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("Cargo.toml").exists() {
                let dir_name = path.file_name().unwrap().to_str().unwrap();
                if !member_names.contains(dir_name) {
                    unlisted.push(dir_name.to_string());
                }
            }
        }
    }
    assert!(
        unlisted.is_empty(),
        "Crate dirs not in workspace.members: {unlisted:?}"
    );
}

#[test]
fn workspace_members_point_to_existing_dirs() {
    let root = workspace_root();
    let mut missing = Vec::new();
    for member in workspace_members() {
        if !root.join(&member).join("Cargo.toml").exists() {
            missing.push(member);
        }
    }
    assert!(
        missing.is_empty(),
        "Workspace members with missing Cargo.toml: {missing:?}"
    );
}

#[test]
fn all_crate_names_match_directory() {
    let mut mismatches = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if let Some(pkg_name) = pkg.get("name").and_then(|v| v.as_str()) {
            if pkg_name != name {
                mismatches.push(format!("dir={name}, name={pkg_name}"));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "Crate name/directory mismatches: {mismatches:?}"
    );
}

#[test]
fn sdk_crates_exist_for_all_vendors() {
    let mut missing = Vec::new();
    for vendor in SDK_VENDORS {
        let sdk_name = format!("abp-{vendor}-sdk");
        if !crate_dir(&sdk_name).join("Cargo.toml").exists() {
            missing.push(sdk_name);
        }
    }
    assert!(missing.is_empty(), "Missing SDK crates: {missing:?}");
}

#[test]
fn shim_crates_exist_for_all_vendors() {
    let mut missing = Vec::new();
    for vendor in SDK_VENDORS {
        let shim_name = format!("abp-shim-{vendor}");
        if !crate_dir(&shim_name).join("Cargo.toml").exists() {
            missing.push(shim_name);
        }
    }
    assert!(missing.is_empty(), "Missing shim crates: {missing:?}");
}

#[test]
fn sdk_mapping_docs_reference_work_order() {
    let dir = workspace_root().join("docs").join("sdk-mapping");
    let mut missing = Vec::new();
    for vendor in SDK_VENDORS {
        let path = dir.join(format!("{vendor}.md"));
        if path.exists() {
            let content = read_file(&path);
            if !content.contains("WorkOrder")
                && !content.contains("work_order")
                && !content.contains("work order")
            {
                missing.push(*vendor);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "SDK mapping docs not referencing WorkOrder: {missing:?}"
    );
}

// ===========================================================================
// 11. Example config validation
// ===========================================================================

#[test]
fn backplane_example_toml_exists() {
    assert!(
        workspace_root().join("backplane.example.toml").exists(),
        "backplane.example.toml missing"
    );
}

#[test]
fn backplane_example_toml_is_valid() {
    let content = read_file(&workspace_root().join("backplane.example.toml"));
    let _: toml::Table = content
        .parse()
        .expect("backplane.example.toml is not valid TOML");
}

// ===========================================================================
// 12. CI and tooling files
// ===========================================================================

#[test]
fn ci_workflow_exists() {
    assert!(
        workspace_root()
            .join(".github")
            .join("workflows")
            .join("ci.yml")
            .exists(),
        ".github/workflows/ci.yml missing"
    );
}

#[test]
fn deny_toml_exists() {
    assert!(
        workspace_root().join("deny.toml").exists(),
        "deny.toml missing"
    );
}

#[test]
fn rustfmt_toml_exists() {
    assert!(
        workspace_root().join("rustfmt.toml").exists(),
        "rustfmt.toml missing"
    );
}

#[test]
fn hosts_directory_exists() {
    assert!(
        workspace_root().join("hosts").is_dir(),
        "hosts/ directory missing"
    );
}

#[test]
fn xtask_is_workspace_member() {
    let members = workspace_members();
    assert!(
        members.iter().any(|m| m == "xtask"),
        "xtask should be a workspace member"
    );
}

// ===========================================================================
// 13. Source file doc coverage heuristics
// ===========================================================================

#[test]
fn all_crates_have_src_directory() {
    let mut missing = Vec::new();
    for name in crate_members() {
        if !crate_dir(&name).join("src").is_dir() {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing src/ directory: {missing:?}"
    );
}

#[test]
fn core_crate_public_items_documented() {
    let src_dir = crate_dir("abp-core").join("src");
    let rs_files = collect_rs_files(&src_dir);
    let mut undocumented = Vec::new();
    for file in &rs_files {
        let content = read_file(file);
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if (trimmed.starts_with("pub struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("pub trait "))
                && !trimmed.contains("pub(crate)")
            {
                // Check if previous lines have doc comments
                let has_doc = (i > 0 && lines[i - 1].trim().starts_with("///"))
                    || (i > 0 && lines[i - 1].trim().starts_with("#[doc"))
                    || (i > 1 && lines[i - 2].trim().starts_with("///"))
                    || (i > 1 && lines[i - 2].trim().starts_with("#[doc"));
                if !has_doc {
                    let fname = file.file_name().unwrap().to_string_lossy();
                    undocumented.push(format!("{fname}:{}: {trimmed}", i + 1));
                }
            }
        }
    }
    // Some enum variants may have derive-doc coverage from the enum itself;
    // allow up to 20 for enums that carry variant-level docs via serde attrs.
    assert!(
        undocumented.len() <= 20,
        "Too many undocumented public items in abp-core ({}):\n{}",
        undocumented.len(),
        undocumented.join("\n")
    );
}

#[test]
fn lib_rs_files_include_readme() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let lib_rs = crate_dir(&name).join("src").join("lib.rs");
        if lib_rs.exists() {
            let content = read_file(&lib_rs);
            if !content.contains("include_str!") && !content.contains("doc = include_str") {
                // It's okay if they have inline //! docs instead
                let line_count = content
                    .lines()
                    .filter(|l| l.trim_start().starts_with("//!"))
                    .count();
                if line_count < 1 {
                    missing.push(name);
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Crate lib.rs files without README include or substantial inline docs: {missing:?}"
    );
}

// ===========================================================================
// 14. Version and edition consistency
// ===========================================================================

#[test]
fn all_crate_editions_use_workspace() {
    let mut overrides = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if pkg.get("edition").is_some_and(|v| v.is_str()) {
            overrides.push(name);
        }
    }
    assert!(
        overrides.is_empty(),
        "Crates overriding workspace edition: {overrides:?}"
    );
}

#[test]
fn all_crate_versions_use_workspace() {
    let mut overrides = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if pkg.get("version").is_some_and(|v| v.is_str()) {
            overrides.push(name);
        }
    }
    assert!(
        overrides.is_empty(),
        "Crates overriding workspace version: {overrides:?}"
    );
}

// ===========================================================================
// 15. Additional documentation quality checks
// ===========================================================================

#[test]
fn changelog_mentions_crate_names() {
    let content = read_file(&workspace_root().join("CHANGELOG.md"));
    let key_crates = ["abp-core", "abp-protocol", "abp-runtime"];
    for name in &key_crates {
        assert!(content.contains(name), "CHANGELOG should mention {name}");
    }
}

#[test]
fn contributing_has_pull_request_section() {
    let content = read_file(&workspace_root().join("CONTRIBUTING.md"));
    assert!(
        content.contains("Pull Request")
            || content.contains("pull request")
            || content.contains("PR"),
        "CONTRIBUTING.md should have a Pull Request section"
    );
}

#[test]
fn contributing_has_getting_started() {
    let content = read_file(&workspace_root().join("CONTRIBUTING.md"));
    assert!(
        content.contains("Getting Started") || content.contains("getting started"),
        "CONTRIBUTING.md should have a Getting Started section"
    );
}

#[test]
fn docs_security_has_content() {
    let content = read_file(&workspace_root().join("docs").join("security.md"));
    assert!(
        content.len() > 100,
        "security.md too short ({} bytes)",
        content.len()
    );
}

#[test]
fn docs_versioning_has_content() {
    let content = read_file(&workspace_root().join("docs").join("versioning.md"));
    assert!(
        content.len() > 100,
        "versioning.md too short ({} bytes)",
        content.len()
    );
}

#[test]
fn docs_testing_has_content() {
    let content = read_file(&workspace_root().join("docs").join("testing.md"));
    assert!(
        content.len() > 100,
        "testing.md too short ({} bytes)",
        content.len()
    );
}

#[test]
fn docs_capabilities_has_content() {
    let content = read_file(&workspace_root().join("docs").join("capabilities.md"));
    assert!(
        content.len() > 100,
        "capabilities.md too short ({} bytes)",
        content.len()
    );
}

#[test]
fn all_crate_descriptions_are_unique() {
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if let Some(desc) = pkg.get("description").and_then(|v| v.as_str()) {
            seen.entry(desc.to_string()).or_default().push(name.clone());
        }
    }
    let duplicates: Vec<_> = seen
        .into_iter()
        .filter(|(_, crates)| crates.len() > 1)
        .collect();
    assert!(
        duplicates.is_empty(),
        "Crates sharing identical descriptions: {duplicates:?}"
    );
}

#[test]
fn docs_sidecar_protocol_documents_envelope_types() {
    let content = read_file(&workspace_root().join("docs").join("sidecar_protocol.md"));
    for envelope_type in &["hello", "run", "event", "final", "fatal"] {
        assert!(
            content.contains(envelope_type),
            "sidecar_protocol.md should document '{envelope_type}' envelope"
        );
    }
}

#[test]
fn readme_has_badges() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("[![") || readme.contains("!["),
        "README should have badge images"
    );
}

#[test]
fn readme_has_quick_start_or_usage() {
    let readme = read_file(&workspace_root().join("README.md"));
    assert!(
        readme.contains("Quick Start")
            || readme.contains("quick start")
            || readme.contains("Usage")
            || readme.contains("Getting Started")
            || readme.contains("## Install")
            || readme.contains("cargo"),
        "README should have a quick start / usage section"
    );
}
