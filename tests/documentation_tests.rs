//! Documentation consistency and completeness tests.
//!
//! Validates that workspace metadata, docs structure, README references,
//! SDK mapping docs, schema files, and configuration examples are all
//! consistent and complete.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn crate_dir(name: &str) -> PathBuf {
    workspace_root().join("crates").join(name)
}

fn read_root_cargo_toml() -> toml::Table {
    let path = workspace_root().join("Cargo.toml");
    let content = std::fs::read_to_string(&path).expect("read root Cargo.toml");
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

/// Crate name extracted from a workspace member path like `crates/abp-core`.
fn member_crate_name(member: &str) -> &str {
    member.rsplit('/').next().unwrap_or(member)
}

fn read_crate_cargo_toml(name: &str) -> toml::Table {
    let path = crate_dir(name).join("Cargo.toml");
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: read error: {e}"));
    content
        .parse()
        .unwrap_or_else(|e| panic!("{name}: parse error: {e}"))
}

fn package_table(table: &toml::Table) -> &toml::Table {
    table["package"]
        .as_table()
        .expect("[package] section missing")
}

fn has_field(pkg: &toml::Table, field: &str) -> bool {
    if let Some(val) = pkg.get(field) {
        if val.is_table() {
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

/// All crate-directory members (those under `crates/`).
fn crate_members() -> Vec<String> {
    workspace_members()
        .into_iter()
        .filter(|m| m.starts_with("crates/"))
        .map(|m| member_crate_name(&m).to_string())
        .collect()
}

/// The six vendor SDKs tracked by the project.
const SDK_VENDORS: &[&str] = &["claude", "codex", "copilot", "gemini", "kimi", "openai"];

// ---------------------------------------------------------------------------
// README – public types mentioned in README exist as crates
// ---------------------------------------------------------------------------

#[test]
fn readme_exists_and_has_content() {
    let readme = workspace_root().join("README.md");
    assert!(readme.exists(), "README.md missing at workspace root");
    let content = std::fs::read_to_string(&readme).unwrap();
    assert!(
        content.len() > 500,
        "README.md seems too short ({} bytes)",
        content.len()
    );
}

#[test]
fn readme_mentions_all_workspace_crates() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    let mut missing = Vec::new();
    for name in crate_members() {
        // xtask and error-taxonomy may not appear; only check crates/ members
        if !readme.contains(&name) {
            missing.push(name);
        }
    }
    // Allow internal/auxiliary crates that may not be individually listed in README
    let internal_crates = [
        "abp-ir",
        "abp-error-taxonomy",
        "abp-shim-codex",
        "abp-shim-copilot",
        "abp-shim-kimi",
    ];
    missing.retain(|n| !internal_crates.contains(&n.as_str()));
    assert!(
        missing.is_empty(),
        "README.md does not mention these crates: {missing:?}"
    );
}

#[test]
fn readme_mentions_work_order_type() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    assert!(
        readme.contains("WorkOrder"),
        "README should mention WorkOrder type"
    );
}

#[test]
fn readme_mentions_receipt_type() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    assert!(
        readme.contains("Receipt"),
        "README should mention Receipt type"
    );
}

#[test]
fn readme_mentions_agent_event_type() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    assert!(
        readme.contains("AgentEvent"),
        "README should mention AgentEvent type"
    );
}

#[test]
fn readme_mentions_envelope_type() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    assert!(
        readme.contains("Envelope"),
        "README should mention Envelope type"
    );
}

#[test]
fn readme_mentions_contract_version() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    assert!(
        readme.contains("abp-core"),
        "README should reference abp-core (the contract crate)"
    );
}

#[test]
fn readme_references_docs_directory() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    assert!(
        readme.contains("docs/"),
        "README should reference docs/ directory"
    );
}

#[test]
fn readme_references_contributing() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    assert!(
        readme.contains("CONTRIBUTING.md"),
        "README should reference CONTRIBUTING.md"
    );
}

#[test]
fn readme_references_sidecar_protocol() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    assert!(
        readme.contains("sidecar_protocol"),
        "README should reference sidecar protocol docs"
    );
}

// ---------------------------------------------------------------------------
// Cargo.toml metadata completeness for all crates
// ---------------------------------------------------------------------------

#[test]
fn all_crates_have_description() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if !has_field(pkg, "description") {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing description: {missing:?}"
    );
}

#[test]
fn all_crates_have_keywords() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if !has_field(pkg, "keywords") {
            missing.push(name);
        }
    }
    assert!(missing.is_empty(), "Crates missing keywords: {missing:?}");
}

#[test]
fn all_crates_have_readme_field() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if !has_field(pkg, "readme") {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing readme field: {missing:?}"
    );
}

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
fn all_crates_have_license() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if !has_field(pkg, "license") {
            missing.push(name);
        }
    }
    assert!(missing.is_empty(), "Crates missing license: {missing:?}");
}

#[test]
fn all_crates_have_repository() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if !has_field(pkg, "repository") {
            missing.push(name);
        }
    }
    assert!(missing.is_empty(), "Crates missing repository: {missing:?}");
}

#[test]
fn all_crates_have_categories() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if !has_field(pkg, "categories") {
            missing.push(name);
        }
    }
    assert!(missing.is_empty(), "Crates missing categories: {missing:?}");
}

// ---------------------------------------------------------------------------
// Version consistency across Cargo.toml files
// ---------------------------------------------------------------------------

#[test]
fn all_crate_versions_match_workspace() {
    let root = read_root_cargo_toml();
    let ws_version = root["workspace"]["package"]["version"]
        .as_str()
        .expect("workspace.package.version");

    let mut mismatched = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if let Some(s) = pkg.get("version").and_then(|v| v.as_str())
            && s != ws_version
        {
            mismatched.push(format!("{name}: {s}"));
        }
    }
    assert!(
        mismatched.is_empty(),
        "Version mismatches (expected {ws_version}): {mismatched:?}"
    );
}

#[test]
fn workspace_package_has_version() {
    let root = read_root_cargo_toml();
    assert!(
        root["workspace"]["package"]
            .get("version")
            .and_then(|v| v.as_str())
            .is_some(),
        "workspace.package.version must be set"
    );
}

#[test]
fn workspace_package_has_edition() {
    let root = read_root_cargo_toml();
    assert!(
        root["workspace"]["package"]
            .get("edition")
            .and_then(|v| v.as_str())
            .is_some(),
        "workspace.package.edition must be set"
    );
}

// ---------------------------------------------------------------------------
// License consistency across all crates
// ---------------------------------------------------------------------------

#[test]
fn license_files_exist_at_root() {
    let root = workspace_root();
    assert!(root.join("LICENSE-MIT").exists(), "LICENSE-MIT missing");
    assert!(
        root.join("LICENSE-APACHE").exists(),
        "LICENSE-APACHE missing"
    );
}

#[test]
fn license_files_have_content() {
    for name in &["LICENSE-MIT", "LICENSE-APACHE"] {
        let path = workspace_root().join(name);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.len() > 100,
            "{name} seems too short ({} bytes)",
            content.len()
        );
    }
}

#[test]
fn all_crate_licenses_are_dual() {
    let root = read_root_cargo_toml();
    let ws_license = root["workspace"]["package"]["license"]
        .as_str()
        .expect("workspace license");

    // Workspace license should be dual MIT/Apache
    assert!(
        ws_license.contains("MIT") && ws_license.contains("Apache"),
        "Workspace license should be dual MIT/Apache, got: {ws_license}"
    );
}

#[test]
fn no_crate_overrides_workspace_license() {
    let mut overrides = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        // If license is set directly (not via workspace), that's an override
        if pkg.get("license").is_some_and(|val| val.is_str()) {
            overrides.push(name);
        }
    }
    assert!(
        overrides.is_empty(),
        "Crates overriding workspace license (should use workspace = true): {overrides:?}"
    );
}

// ---------------------------------------------------------------------------
// docs/ directory structure validation
// ---------------------------------------------------------------------------

#[test]
fn docs_directory_exists() {
    assert!(
        workspace_root().join("docs").is_dir(),
        "docs/ directory missing"
    );
}

#[test]
fn docs_architecture_exists() {
    assert!(
        workspace_root()
            .join("docs")
            .join("architecture.md")
            .exists(),
        "docs/architecture.md missing"
    );
}

#[test]
fn docs_sidecar_protocol_exists() {
    assert!(
        workspace_root()
            .join("docs")
            .join("sidecar_protocol.md")
            .exists(),
        "docs/sidecar_protocol.md missing"
    );
}

#[test]
fn docs_sdk_mapping_exists() {
    assert!(
        workspace_root()
            .join("docs")
            .join("sdk_mapping.md")
            .exists(),
        "docs/sdk_mapping.md missing"
    );
}

#[test]
fn docs_capabilities_exists() {
    assert!(
        workspace_root()
            .join("docs")
            .join("capabilities.md")
            .exists(),
        "docs/capabilities.md missing"
    );
}

#[test]
fn docs_security_exists() {
    assert!(
        workspace_root().join("docs").join("security.md").exists(),
        "docs/security.md missing"
    );
}

#[test]
fn docs_versioning_exists() {
    assert!(
        workspace_root().join("docs").join("versioning.md").exists(),
        "docs/versioning.md missing"
    );
}

#[test]
fn docs_errors_exists() {
    assert!(
        workspace_root().join("docs").join("errors.md").exists()
            || workspace_root()
                .join("docs")
                .join("error_codes.md")
                .exists(),
        "docs/errors.md or docs/error_codes.md missing"
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
fn docs_dialect_engine_matrix_exists() {
    assert!(
        workspace_root()
            .join("docs")
            .join("dialect_engine_matrix.md")
            .exists(),
        "docs/dialect_engine_matrix.md missing"
    );
}

// ---------------------------------------------------------------------------
// SDK mapping docs exist for all 6 SDKs
// ---------------------------------------------------------------------------

#[test]
fn sdk_mapping_doc_exists_for_claude() {
    assert!(
        workspace_root()
            .join("docs")
            .join("sdk-mapping")
            .join("claude.md")
            .exists(),
        "docs/sdk-mapping/claude.md missing"
    );
}

#[test]
fn sdk_mapping_doc_exists_for_codex() {
    assert!(
        workspace_root()
            .join("docs")
            .join("sdk-mapping")
            .join("codex.md")
            .exists(),
        "docs/sdk-mapping/codex.md missing"
    );
}

#[test]
fn sdk_mapping_doc_exists_for_copilot() {
    assert!(
        workspace_root()
            .join("docs")
            .join("sdk-mapping")
            .join("copilot.md")
            .exists(),
        "docs/sdk-mapping/copilot.md missing"
    );
}

#[test]
fn sdk_mapping_doc_exists_for_gemini() {
    assert!(
        workspace_root()
            .join("docs")
            .join("sdk-mapping")
            .join("gemini.md")
            .exists(),
        "docs/sdk-mapping/gemini.md missing"
    );
}

#[test]
fn sdk_mapping_doc_exists_for_kimi() {
    assert!(
        workspace_root()
            .join("docs")
            .join("sdk-mapping")
            .join("kimi.md")
            .exists(),
        "docs/sdk-mapping/kimi.md missing"
    );
}

#[test]
fn sdk_mapping_doc_exists_for_openai() {
    assert!(
        workspace_root()
            .join("docs")
            .join("sdk-mapping")
            .join("openai.md")
            .exists(),
        "docs/sdk-mapping/openai.md missing"
    );
}

#[test]
fn sdk_mapping_docs_have_content() {
    let dir = workspace_root().join("docs").join("sdk-mapping");
    let mut empty = Vec::new();
    for vendor in SDK_VENDORS {
        let path = dir.join(format!("{vendor}.md"));
        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap();
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
fn sdk_mapping_matrix_exists() {
    assert!(
        workspace_root()
            .join("docs")
            .join("sdk-mapping")
            .join("mapping-matrix.md")
            .exists(),
        "docs/sdk-mapping/mapping-matrix.md missing"
    );
}

// ---------------------------------------------------------------------------
// Schema files exist in contracts/schemas/
// ---------------------------------------------------------------------------

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
        "contracts/schemas/work_order.schema.json missing"
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
        "contracts/schemas/receipt.schema.json missing"
    );
}

#[test]
fn schema_files_are_valid_json() {
    let dir = workspace_root().join("contracts").join("schemas");
    let mut failures = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let content = std::fs::read_to_string(&path).unwrap();
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
fn schema_files_have_schema_keyword() {
    let dir = workspace_root().join("contracts").join("schemas");
    let mut failures = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let content = std::fs::read_to_string(&path).unwrap();
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    // JSON Schema files should have a $schema or title key
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

// ---------------------------------------------------------------------------
// Example configs are valid TOML
// ---------------------------------------------------------------------------

#[test]
fn backplane_example_toml_exists() {
    assert!(
        workspace_root().join("backplane.example.toml").exists(),
        "backplane.example.toml missing"
    );
}

#[test]
fn backplane_example_toml_is_valid() {
    let path = workspace_root().join("backplane.example.toml");
    let content = std::fs::read_to_string(&path).expect("read backplane.example.toml");
    let _: toml::Table = content
        .parse()
        .expect("backplane.example.toml is not valid TOML");
}

#[test]
fn backplane_example_toml_has_backends_section() {
    let path = workspace_root().join("backplane.example.toml");
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("[backends"),
        "backplane.example.toml should have a [backends] section"
    );
}

// ---------------------------------------------------------------------------
// No broken internal doc references in README
// ---------------------------------------------------------------------------

#[test]
fn readme_doc_links_point_to_existing_files() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    let mut broken = Vec::new();

    // Match markdown links like [text](path)
    for cap in readme.split("](") {
        if let Some(end) = cap.find(')') {
            let link = &cap[..end];
            // Only check local file links (not http, not anchors)
            if !link.starts_with("http")
                && !link.starts_with('#')
                && !link.starts_with("mailto:")
                && !link.contains("://")
            {
                // Strip anchor fragments
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
fn docs_files_are_non_empty() {
    let docs_dir = workspace_root().join("docs");
    let mut empty = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&docs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "md") {
                let content = std::fs::read_to_string(&path).unwrap();
                if content.trim().is_empty() {
                    empty.push(path.display().to_string());
                }
            }
        }
    }
    assert!(empty.is_empty(), "Empty doc files: {empty:?}");
}

// ---------------------------------------------------------------------------
// CHANGELOG.md and CONTRIBUTING.md
// ---------------------------------------------------------------------------

#[test]
fn changelog_exists() {
    assert!(
        workspace_root().join("CHANGELOG.md").exists(),
        "CHANGELOG.md missing"
    );
}

#[test]
fn changelog_has_content() {
    let content =
        std::fs::read_to_string(workspace_root().join("CHANGELOG.md")).expect("read CHANGELOG.md");
    assert!(
        content.len() > 50,
        "CHANGELOG.md is too short ({} bytes)",
        content.len()
    );
}

#[test]
fn changelog_has_version_entry() {
    let content =
        std::fs::read_to_string(workspace_root().join("CHANGELOG.md")).expect("read CHANGELOG.md");
    // Should contain at least one version header like ## [0.1.0]
    assert!(
        content.contains("## ["),
        "CHANGELOG.md should contain at least one version entry (## [x.y.z])"
    );
}

#[test]
fn contributing_exists() {
    assert!(
        workspace_root().join("CONTRIBUTING.md").exists(),
        "CONTRIBUTING.md missing"
    );
}

#[test]
fn contributing_has_content() {
    let content = std::fs::read_to_string(workspace_root().join("CONTRIBUTING.md"))
        .expect("read CONTRIBUTING.md");
    assert!(
        content.len() > 100,
        "CONTRIBUTING.md is too short ({} bytes)",
        content.len()
    );
}

#[test]
fn code_of_conduct_exists() {
    assert!(
        workspace_root().join("CODE_OF_CONDUCT.md").exists(),
        "CODE_OF_CONDUCT.md missing"
    );
}

// ---------------------------------------------------------------------------
// All workspace members listed in root Cargo.toml
// ---------------------------------------------------------------------------

#[test]
fn all_crate_dirs_are_workspace_members() {
    let members = workspace_members();
    let member_names: Vec<&str> = members.iter().map(|m| member_crate_name(m)).collect();

    let crates_dir = workspace_root().join("crates");
    let mut unlisted = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&crates_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("Cargo.toml").exists() {
                let dir_name = path.file_name().unwrap().to_str().unwrap();
                if !member_names.contains(&dir_name) {
                    unlisted.push(dir_name.to_string());
                }
            }
        }
    }
    assert!(
        unlisted.is_empty(),
        "Crate directories not in workspace.members: {unlisted:?}"
    );
}

#[test]
fn workspace_members_point_to_existing_dirs() {
    let root = workspace_root();
    let mut missing = Vec::new();
    for member in workspace_members() {
        let dir = root.join(&member);
        if !dir.join("Cargo.toml").exists() {
            missing.push(member);
        }
    }
    assert!(
        missing.is_empty(),
        "Workspace members with missing Cargo.toml: {missing:?}"
    );
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

// ---------------------------------------------------------------------------
// No publish=false on library crates (except daemon, cli, and xtask)
// ---------------------------------------------------------------------------

/// Crates allowed to have `publish = false`.
const PUBLISH_FALSE_ALLOWED: &[&str] = &["abp-daemon", "abp-cli"];

#[test]
fn no_unexpected_publish_false() {
    let mut violations = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if pkg.get("publish").and_then(|v| v.as_bool()) == Some(false)
            && !PUBLISH_FALSE_ALLOWED.contains(&name.as_str())
        {
            violations.push(name);
        }
    }
    assert!(
        violations.is_empty(),
        "Library crates with unexpected publish = false: {violations:?}"
    );
}

// ---------------------------------------------------------------------------
// SDK crate existence
// ---------------------------------------------------------------------------

#[test]
fn sdk_crates_exist_for_all_vendors() {
    let mut missing = Vec::new();
    for vendor in SDK_VENDORS {
        let sdk_name = format!("abp-{vendor}-sdk");
        if !crate_dir(&sdk_name).join("Cargo.toml").exists() {
            missing.push(sdk_name);
        }
    }
    assert!(
        missing.is_empty(),
        "Missing SDK crates for vendors: {missing:?}"
    );
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
    assert!(
        missing.is_empty(),
        "Missing shim crates for vendors: {missing:?}"
    );
}

// ---------------------------------------------------------------------------
// Cross-reference: SDK mapping docs reference valid types
// ---------------------------------------------------------------------------

#[test]
fn sdk_mapping_docs_reference_work_order() {
    let dir = workspace_root().join("docs").join("sdk-mapping");
    let mut missing = Vec::new();
    for vendor in SDK_VENDORS {
        let path = dir.join(format!("{vendor}.md"));
        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap();
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

#[test]
fn sdk_mapping_docs_reference_receipt() {
    let dir = workspace_root().join("docs").join("sdk-mapping");
    let mut missing = Vec::new();
    for vendor in SDK_VENDORS {
        let path = dir.join(format!("{vendor}.md"));
        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap();
            if !content.contains("Receipt")
                && !content.contains("receipt")
                && !content.contains("response")
            {
                missing.push(*vendor);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "SDK mapping docs not referencing Receipt/response: {missing:?}"
    );
}

// ---------------------------------------------------------------------------
// Workspace metadata completeness
// ---------------------------------------------------------------------------

#[test]
fn workspace_package_has_authors() {
    let root = read_root_cargo_toml();
    assert!(
        root["workspace"]["package"].get("authors").is_some(),
        "workspace.package.authors must be set"
    );
}

#[test]
fn workspace_package_has_repository() {
    let root = read_root_cargo_toml();
    assert!(
        root["workspace"]["package"]
            .get("repository")
            .and_then(|v| v.as_str())
            .is_some(),
        "workspace.package.repository must be set"
    );
}

#[test]
fn workspace_package_has_license() {
    let root = read_root_cargo_toml();
    assert!(
        root["workspace"]["package"]
            .get("license")
            .and_then(|v| v.as_str())
            .is_some(),
        "workspace.package.license must be set"
    );
}

#[test]
fn workspace_package_has_description() {
    let root = read_root_cargo_toml();
    assert!(
        root["workspace"]["package"]
            .get("description")
            .and_then(|v| v.as_str())
            .is_some(),
        "workspace.package.description must be set"
    );
}

// ---------------------------------------------------------------------------
// Additional structural checks
// ---------------------------------------------------------------------------

#[test]
fn all_crates_have_src_directory() {
    let mut missing = Vec::new();
    for name in crate_members() {
        let src = crate_dir(&name).join("src");
        if !src.is_dir() {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing src/ directory: {missing:?}"
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
fn keywords_within_crates_io_limits() {
    let mut violations = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if let Some(kw) = pkg.get("keywords").and_then(|v| v.as_array()) {
            if kw.len() > 5 {
                violations.push(format!("{name}: {} keywords", kw.len()));
            }
            for k in kw {
                let s = k.as_str().unwrap_or("");
                if s.len() > 20 {
                    violations.push(format!("{name}: keyword '{s}' > 20 chars"));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Keywords exceeding crates.io limits: {violations:?}"
    );
}

#[test]
fn all_crate_names_match_directory() {
    let mut mismatches = Vec::new();
    for name in crate_members() {
        let table = read_crate_cargo_toml(&name);
        let pkg = package_table(&table);
        if let Some(pkg_name) = pkg.get("name").and_then(|v| v.as_str())
            && pkg_name != name
        {
            mismatches.push(format!("dir={name}, name={pkg_name}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "Crate name/directory mismatches: {mismatches:?}"
    );
}

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

#[test]
fn ci_workflow_exists() {
    let ci = workspace_root()
        .join(".github")
        .join("workflows")
        .join("ci.yml");
    assert!(ci.exists(), ".github/workflows/ci.yml missing");
}

#[test]
fn hosts_directory_exists() {
    assert!(
        workspace_root().join("hosts").is_dir(),
        "hosts/ directory missing"
    );
}

#[test]
fn deny_toml_exists() {
    assert!(
        workspace_root().join("deny.toml").exists(),
        "deny.toml missing for cargo-deny"
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

#[test]
fn backplane_config_schema_exists() {
    assert!(
        workspace_root()
            .join("contracts")
            .join("schemas")
            .join("backplane_config.schema.json")
            .exists(),
        "contracts/schemas/backplane_config.schema.json missing"
    );
}

#[test]
fn docs_testing_exists() {
    assert!(
        workspace_root().join("docs").join("testing.md").exists(),
        "docs/testing.md missing"
    );
}

#[test]
fn docs_capability_negotiation_exists() {
    assert!(
        workspace_root()
            .join("docs")
            .join("capability_negotiation.md")
            .exists(),
        "docs/capability_negotiation.md missing"
    );
}
