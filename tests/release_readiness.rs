//! Release readiness checks for crates.io publication.
//!
//! Validates that every workspace crate has the metadata, documentation,
//! and structural requirements needed for a successful `cargo publish`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

// ── Helpers ──────────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_toml(path: &Path) -> toml::Value {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    content
        .parse::<toml::Value>()
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()))
}

fn workspace_toml() -> toml::Value {
    read_toml(&workspace_root().join("Cargo.toml"))
}

fn workspace_member_dirs() -> Vec<String> {
    let cargo = workspace_toml();
    cargo["workspace"]["members"]
        .as_array()
        .expect("workspace.members should be an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn resolve_pkg_field(
    crate_toml: &toml::Value,
    ws_toml: &toml::Value,
    field: &str,
) -> Option<String> {
    let val = crate_toml.get("package")?.get(field)?;
    if let Some(s) = val.as_str() {
        return Some(s.to_string());
    }
    if val.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
        return ws_toml
            .get("workspace")?
            .get("package")?
            .get(field)?
            .as_str()
            .map(|s| s.to_string());
    }
    None
}

fn resolve_pkg_array(
    crate_toml: &toml::Value,
    ws_toml: &toml::Value,
    field: &str,
) -> Option<Vec<String>> {
    let pkg = crate_toml.get("package")?;
    let val = pkg.get(field)?;
    if let Some(arr) = val.as_array() {
        return Some(
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        );
    }
    if val.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
        return ws_toml
            .get("workspace")?
            .get("package")?
            .get(field)?
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });
    }
    None
}

/// Crate names that are intentionally `publish = false`.
fn publish_false_allowed() -> HashSet<&'static str> {
    ["abp-cli", "abp-daemon", "xtask"].into_iter().collect()
}

/// Build-tool crate names exempt from library-level checks.
fn build_tool_names() -> HashSet<&'static str> {
    ["xtask"].into_iter().collect()
}

// ── Valid crates.io categories (subset of the taxonomy) ─────────────────

fn valid_categories() -> HashSet<&'static str> {
    [
        "accessibility",
        "aerospace",
        "algorithms",
        "api-bindings",
        "asynchronous",
        "authentication",
        "caching",
        "command-line-interface",
        "command-line-utilities",
        "compilers",
        "compression",
        "concurrency",
        "config",
        "cryptography",
        "data-structures",
        "database",
        "database-implementations",
        "date-and-time",
        "development-tools",
        "development-tools::build-utils",
        "development-tools::cargo-plugins",
        "development-tools::debugging",
        "development-tools::ffi",
        "development-tools::procedural-macro-helpers",
        "development-tools::profiling",
        "development-tools::testing",
        "email",
        "embedded",
        "emulators",
        "encoding",
        "external-ffi-bindings",
        "filesystem",
        "game-engines",
        "games",
        "graphics",
        "gui",
        "hardware-support",
        "internationalization",
        "mathematics",
        "memory-management",
        "multimedia",
        "network-programming",
        "no-std",
        "os",
        "parser-implementations",
        "parsing",
        "rendering",
        "rust-patterns",
        "science",
        "simulation",
        "template-engine",
        "text-editors",
        "text-processing",
        "visualization",
        "wasm",
        "web-programming",
        "web-programming::http-client",
        "web-programming::http-server",
        "web-programming::websocket",
    ]
    .into_iter()
    .collect()
}

// ── Tests ────────────────────────────────────────────────────────────────

#[test]
fn all_crates_have_valid_cargo_toml_metadata() {
    let root = workspace_root();
    let ws = workspace_toml();
    let skip = build_tool_names();
    let required_fields = [
        "name",
        "version",
        "edition",
        "description",
        "license",
        "repository",
    ];
    let mut errors: Vec<String> = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        if skip.contains(name.as_str()) {
            continue;
        }
        for field in &required_fields {
            if resolve_pkg_field(&ct, &ws, field).is_none_or(|s| s.is_empty()) {
                errors.push(format!("{name}: missing or empty `{field}`"));
            }
        }
    }
    assert!(
        errors.is_empty(),
        "Cargo.toml metadata issues:\n  {}",
        errors.join("\n  ")
    );
}

#[test]
fn all_crates_have_readme() {
    let root = workspace_root();
    let skip = build_tool_names();
    let mut missing = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        if skip.contains(name.as_str()) {
            continue;
        }
        let has_readme_field = ct["package"]
            .get("readme")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        let has_readme_file = root.join(&dir).join("README.md").exists();

        if !has_readme_field && !has_readme_file {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing readme field or README.md:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn all_crates_have_license() {
    let root = workspace_root();
    let ws = workspace_toml();
    let mut bad = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        match resolve_pkg_field(&ct, &ws, "license") {
            Some(ref lic) if lic == "MIT OR Apache-2.0" => {}
            other => bad.push(format!("{name}: {:?}", other)),
        }
    }
    assert!(
        bad.is_empty(),
        "Crates with wrong/missing license:\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn all_crates_have_repository() {
    let root = workspace_root();
    let ws = workspace_toml();
    let mut missing = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        if resolve_pkg_field(&ct, &ws, "repository").is_none_or(|s| s.is_empty()) {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing repository:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn all_lib_rs_have_warn_missing_docs() {
    let root = workspace_root();
    let mut missing = Vec::new();

    for dir in workspace_member_dirs() {
        let lib_rs = root.join(&dir).join("src").join("lib.rs");
        if !lib_rs.exists() {
            continue;
        }
        let content = fs::read_to_string(&lib_rs).unwrap();
        if !content.contains("warn(missing_docs)") {
            let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
            let name = ct["package"]["name"].as_str().unwrap_or("?");
            missing.push(name.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "lib.rs missing #![warn(missing_docs)]:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn all_lib_rs_have_deny_unsafe_code() {
    let root = workspace_root();
    let mut missing = Vec::new();

    for dir in workspace_member_dirs() {
        let lib_rs = root.join(&dir).join("src").join("lib.rs");
        if !lib_rs.exists() {
            continue;
        }
        let content = fs::read_to_string(&lib_rs).unwrap();
        if !content.contains("deny(unsafe_code)") {
            let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
            let name = ct["package"]["name"].as_str().unwrap_or("?");
            missing.push(name.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "lib.rs missing #![deny(unsafe_code)]:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn no_unintended_publish_false() {
    let root = workspace_root();
    let allowed = publish_false_allowed();
    let mut bad = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();

        let publish_false = ct["package"]
            .get("publish")
            .and_then(|v| v.as_bool())
            .is_some_and(|b| !b);

        if publish_false && !allowed.contains(name.as_str()) {
            bad.push(name);
        }
    }
    assert!(
        bad.is_empty(),
        "Unexpected publish = false (allowed: {allowed:?}):\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn version_consistency() {
    let root = workspace_root();
    let ws = workspace_toml();
    let expected = ws["workspace"]["package"]["version"]
        .as_str()
        .expect("workspace.package.version");
    let mut mismatched = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        let ver = resolve_pkg_field(&ct, &ws, "version");
        if ver.as_deref() != Some(expected) {
            mismatched.push(format!("{name}: {:?}", ver));
        }
    }
    assert!(
        mismatched.is_empty(),
        "Version mismatch (expected {expected}):\n  {}",
        mismatched.join("\n  ")
    );
}

#[test]
fn keywords_are_nonempty() {
    let root = workspace_root();
    let ws = workspace_toml();
    let skip = build_tool_names();
    let mut missing = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        if skip.contains(name.as_str()) {
            continue;
        }
        let kw = resolve_pkg_array(&ct, &ws, "keywords");
        if kw.is_none_or(|v| v.is_empty()) {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "Crates with empty or missing keywords:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn categories_match_crates_io_taxonomy() {
    let root = workspace_root();
    let ws = workspace_toml();
    let skip = build_tool_names();
    let valid = valid_categories();
    let mut errors: Vec<String> = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        if skip.contains(name.as_str()) {
            continue;
        }
        if let Some(cats) = resolve_pkg_array(&ct, &ws, "categories") {
            for cat in &cats {
                if !valid.contains(cat.as_str()) {
                    errors.push(format!("{name}: invalid category `{cat}`"));
                }
            }
            if cats.is_empty() {
                errors.push(format!("{name}: categories array is empty"));
            }
        } else {
            errors.push(format!("{name}: missing categories"));
        }
    }
    assert!(
        errors.is_empty(),
        "Category issues:\n  {}",
        errors.join("\n  ")
    );
}

#[test]
fn all_descriptions_are_nonempty() {
    let root = workspace_root();
    let ws = workspace_toml();
    let skip = build_tool_names();
    let mut missing = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        if skip.contains(name.as_str()) {
            continue;
        }
        if resolve_pkg_field(&ct, &ws, "description").is_none_or(|s| s.is_empty()) {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "Crates with empty/missing description:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn no_path_only_deps_without_version() {
    let root = workspace_root();
    let members = workspace_member_dirs();
    let mut bad = Vec::new();

    let member_paths: Vec<PathBuf> = members
        .iter()
        .map(|m| {
            let p = root.join(m);
            p.canonicalize().unwrap_or(p)
        })
        .collect();

    for dir in &members {
        let ct = read_toml(&root.join(dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();

        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            let deps = match ct.get(section).and_then(|v| v.as_table()) {
                Some(d) => d,
                None => continue,
            };

            for (dep_name, dep_val) in deps {
                let is_workspace = dep_val
                    .get("workspace")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let has_version = dep_val.get("version").is_some() || dep_val.is_str();
                let dep_path = dep_val.get("path").and_then(|v| v.as_str());

                if let Some(rel) = dep_path
                    && !has_version
                    && !is_workspace
                {
                    let resolved = root.join(dir).join(rel);
                    let resolved = resolved.canonicalize().unwrap_or(resolved);
                    let is_member = member_paths.contains(&resolved);
                    if !is_member {
                        bad.push(format!(
                            "{name}: {dep_name} in [{section}] (path-only, not a workspace member)"
                        ));
                    }
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "Path-only deps without version:\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn no_wildcard_dependencies() {
    let root = workspace_root();
    let ws = workspace_toml();
    let mut bad = Vec::new();

    if let Some(deps) = ws
        .get("workspace")
        .and_then(|v| v.get("dependencies"))
        .and_then(|v| v.as_table())
    {
        for (name, val) in deps {
            let version = val
                .as_str()
                .or_else(|| val.get("version").and_then(|v| v.as_str()));
            if version == Some("*") {
                bad.push(format!("workspace: {name}"));
            }
        }
    }

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let crate_name = ct["package"]["name"].as_str().unwrap_or("?");
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(deps) = ct.get(section).and_then(|v| v.as_table()) {
                for (name, val) in deps {
                    let version = val
                        .as_str()
                        .or_else(|| val.get("version").and_then(|v| v.as_str()));
                    if version == Some("*") {
                        bad.push(format!("{crate_name}: {name}"));
                    }
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "Wildcard dependencies:\n  {}",
        bad.join("\n  ")
    );
}
