//! Release readiness gate tests.
//!
//! Validates workspace metadata, documentation, licensing, and structural
//! requirements that must be satisfied before publishing a release.

use std::collections::{HashMap, HashSet, VecDeque};
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

/// Returns workspace member directory paths (relative to root).
fn workspace_member_dirs() -> Vec<String> {
    let cargo = workspace_toml();
    cargo["workspace"]["members"]
        .as_array()
        .expect("workspace.members should be an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// Resolve a `[package]` string field, following workspace inheritance.
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

/// Crate names that are build tools (exempt from publish-metadata checks).
fn build_tool_names() -> HashSet<&'static str> {
    ["xtask"].into_iter().collect()
}

// ── 1. License files ─────────────────────────────────────────────────────

#[test]
fn license_mit_exists() {
    let p = workspace_root().join("LICENSE-MIT");
    assert!(p.exists(), "LICENSE-MIT must exist at workspace root");
    assert!(fs::metadata(&p).unwrap().len() > 0, "LICENSE-MIT is empty");
}

#[test]
fn license_apache_exists() {
    let p = workspace_root().join("LICENSE-APACHE");
    assert!(p.exists(), "LICENSE-APACHE must exist at workspace root");
    assert!(
        fs::metadata(&p).unwrap().len() > 0,
        "LICENSE-APACHE is empty"
    );
}

// ── 2–3. README / CHANGELOG ─────────────────────────────────────────────

#[test]
fn readme_exists_and_nonempty() {
    let p = workspace_root().join("README.md");
    assert!(p.exists(), "README.md must exist at workspace root");
    assert!(
        fs::metadata(&p).unwrap().len() > 100,
        "README.md should have substantial content"
    );
}

#[test]
fn changelog_exists_and_nonempty() {
    let p = workspace_root().join("CHANGELOG.md");
    assert!(p.exists(), "CHANGELOG.md must exist at workspace root");
    assert!(
        fs::metadata(&p).unwrap().len() > 50,
        "CHANGELOG.md should have substantial content"
    );
}

// ── 4. Every publishable crate has `description` ────────────────────────

#[test]
fn all_crates_have_description() {
    let root = workspace_root();
    let ws = workspace_toml();
    let skip = build_tool_names();
    let mut missing = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?");
        if skip.contains(name) {
            continue;
        }
        if resolve_pkg_field(&ct, &ws, "description").is_none_or(|s| s.is_empty()) {
            missing.push(name.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing description: {missing:?}"
    );
}

// ── 5. License is "MIT OR Apache-2.0" everywhere ────────────────────────

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
            other => bad.push((name, other)),
        }
    }
    assert!(bad.is_empty(), "Crates with wrong/missing license: {bad:?}");
}

// ── 6. Version consistency ──────────────────────────────────────────────

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
            mismatched.push((name, ver));
        }
    }
    assert!(
        mismatched.is_empty(),
        "Version mismatch (expected {expected}): {mismatched:?}"
    );
}

// ── 7. No path-only deps to non-workspace crates ───────────────────────

#[test]
fn no_path_only_external_dependencies() {
    let root = workspace_root();
    let members = workspace_member_dirs();
    let mut bad = Vec::new();

    // Collect canonical paths of all workspace members
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
                            "{name}: {dep_name} (path-only, not a workspace member)"
                        ));
                    }
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "Path-only deps to non-workspace crates: {bad:?}"
    );
}

// ── 8. No publish = false on library crates ─────────────────────────────

#[test]
fn no_publish_false_on_library_crates() {
    let root = workspace_root();
    let allowed_bin_only = build_tool_names();
    let mut bad = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        if allowed_bin_only.contains(name.as_str()) {
            continue;
        }

        let has_lib = root.join(&dir).join("src").join("lib.rs").exists();
        let publish_false = ct["package"]
            .get("publish")
            .and_then(|v| v.as_bool())
            .is_some_and(|b| !b);

        if has_lib && publish_false {
            bad.push(name);
        }
    }
    assert!(
        bad.is_empty(),
        "Library crates with publish = false: {bad:?}"
    );
}

// ── 9. CONTRACT_VERSION format ──────────────────────────────────────────

#[test]
fn contract_version_format() {
    let path = workspace_root()
        .join("crates")
        .join("abp-core")
        .join("src")
        .join("lib.rs");
    let content = fs::read_to_string(&path).expect("abp-core/src/lib.rs");
    let line = content
        .lines()
        .find(|l| l.contains("CONTRACT_VERSION"))
        .expect("CONTRACT_VERSION not found in abp-core/src/lib.rs");

    // Extract string value between quotes
    let start = line.find('"').expect("opening quote") + 1;
    let end = line[start..].find('"').expect("closing quote") + start;
    let version = &line[start..end];

    assert!(
        version.starts_with("abp/v"),
        "CONTRACT_VERSION must start with 'abp/v', got: {version}"
    );
    let rest = &version[5..]; // after "abp/v"
    let parts: Vec<&str> = rest.split('.').collect();
    assert_eq!(
        parts.len(),
        2,
        "CONTRACT_VERSION must be major.minor: {version}"
    );
    for (i, part) in parts.iter().enumerate() {
        assert!(
            !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()),
            "CONTRACT_VERSION segment {i} must be digits: {version}"
        );
    }
}

// ── 10. Schema files exist ──────────────────────────────────────────────

#[test]
fn schema_files_exist() {
    let schemas = workspace_root().join("contracts").join("schemas");
    assert!(schemas.is_dir(), "contracts/schemas/ directory missing");

    let expected = ["work_order.schema.json", "receipt.schema.json"];
    for name in &expected {
        let p = schemas.join(name);
        assert!(p.exists(), "Missing schema file: {name}");
        assert!(
            fs::metadata(&p).unwrap().len() > 10,
            "Schema file {name} is suspiciously small"
        );
    }
}

// ── 11. No TODO/FIXME in public API (lib.rs) ────────────────────────────

#[test]
fn no_todo_fixme_in_public_api() {
    let root = workspace_root();
    let mut found = Vec::new();

    for dir in workspace_member_dirs() {
        let lib_rs = root.join(&dir).join("src").join("lib.rs");
        if !lib_rs.exists() {
            continue;
        }
        let content = fs::read_to_string(&lib_rs).unwrap();
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?");

        for (i, line) in content.lines().enumerate() {
            if line.contains("TODO") || line.contains("FIXME") {
                found.push(format!("{name}/src/lib.rs:{}", i + 1));
            }
        }
    }
    assert!(
        found.is_empty(),
        "TODO/FIXME found in public API: {found:?}"
    );
}

// ── 12. All lib.rs have doc comment ─────────────────────────────────────

#[test]
fn all_lib_rs_have_doc_comment() {
    let root = workspace_root();
    let mut missing = Vec::new();

    for dir in workspace_member_dirs() {
        let lib_rs = root.join(&dir).join("src").join("lib.rs");
        if !lib_rs.exists() {
            continue;
        }
        let content = fs::read_to_string(&lib_rs).unwrap();
        let has_doc = content.lines().take(15).any(|l| {
            let t = l.trim();
            t.starts_with("//!") || t.starts_with("#![doc")
        });
        if !has_doc {
            let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
            let name = ct["package"]["name"].as_str().unwrap_or("?");
            missing.push(name.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "lib.rs files missing doc comment: {missing:?}"
    );
}

// ── 13. Repository URL ──────────────────────────────────────────────────

#[test]
fn all_crates_have_repository() {
    let root = workspace_root();
    let ws = workspace_toml();
    let mut missing = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        if resolve_pkg_field(&ct, &ws, "repository").is_none() {
            missing.push(name);
        }
    }
    assert!(missing.is_empty(), "Crates missing repository: {missing:?}");
}

// ── 14. Keywords ────────────────────────────────────────────────────────

#[test]
fn all_crates_have_keywords() {
    let root = workspace_root();
    let skip = build_tool_names();
    let mut missing = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        if skip.contains(name.as_str()) {
            continue;
        }
        let has_keywords = ct["package"]
            .get("keywords")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty());
        if !has_keywords {
            missing.push(name);
        }
    }
    assert!(missing.is_empty(), "Crates missing keywords: {missing:?}");
}

// ── 15. CI workflow ─────────────────────────────────────────────────────

#[test]
fn ci_workflow_exists() {
    let p = workspace_root()
        .join(".github")
        .join("workflows")
        .join("ci.yml");
    assert!(p.exists(), ".github/workflows/ci.yml must exist");
}

// ── 16. deny.toml ──────────────────────────────────────────────────────

#[test]
fn deny_toml_exists() {
    let p = workspace_root().join("deny.toml");
    assert!(p.exists(), "deny.toml must exist for cargo-deny");
}

// ── 17. No unsafe code ─────────────────────────────────────────────────

#[test]
fn no_unsafe_code_in_core_crates() {
    let root = workspace_root();
    let mut missing_deny = Vec::new();

    for dir in workspace_member_dirs() {
        let lib_rs = root.join(&dir).join("src").join("lib.rs");
        if !lib_rs.exists() {
            continue;
        }
        let content = fs::read_to_string(&lib_rs).unwrap();
        if !content.contains("deny(unsafe_code)") {
            let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
            let name = ct["package"]["name"].as_str().unwrap_or("?");
            missing_deny.push(name.to_string());
        }
    }
    assert!(
        missing_deny.is_empty(),
        "Crates missing #![deny(unsafe_code)]: {missing_deny:?}"
    );
}

// ── 18. Host examples exist ─────────────────────────────────────────────

#[test]
fn example_hosts_exist() {
    let hosts = workspace_root().join("hosts");
    assert!(hosts.is_dir(), "hosts/ directory must exist");

    let expected = ["node", "python"];
    for name in &expected {
        let dir = hosts.join(name);
        assert!(dir.is_dir(), "hosts/{name}/ directory missing");
        let has_files = fs::read_dir(&dir)
            .unwrap()
            .any(|e| e.unwrap().file_type().unwrap().is_file());
        assert!(has_files, "hosts/{name}/ has no files");
    }
}

// ── 19. Edition consistency ─────────────────────────────────────────────

#[test]
fn edition_is_consistent() {
    let root = workspace_root();
    let ws = workspace_toml();
    let expected = ws["workspace"]["package"]["edition"]
        .as_str()
        .expect("workspace.package.edition");
    let mut mismatched = Vec::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        let edition = resolve_pkg_field(&ct, &ws, "edition");
        if edition.as_deref() != Some(expected) {
            mismatched.push((name, edition));
        }
    }
    assert!(
        mismatched.is_empty(),
        "Edition mismatch (expected {expected}): {mismatched:?}"
    );
}

// ── 20. Workspace resolver ──────────────────────────────────────────────

#[test]
fn workspace_resolver_set() {
    let ws = workspace_toml();
    let resolver = ws["workspace"].get("resolver").and_then(|v| v.as_str());
    let edition = ws
        .get("workspace")
        .and_then(|v| v.get("package"))
        .and_then(|v| v.get("edition"))
        .and_then(|v| v.as_str());

    // resolver = "2" explicitly, or edition 2024 which defaults to resolver v2
    let ok = matches!(resolver, Some("2" | "3")) || edition == Some("2024");
    assert!(
        ok,
        "workspace must set resolver = \"2\" (or use edition 2024 default)"
    );
}

// ── 21. No wildcard dependencies ────────────────────────────────────────

#[test]
fn no_wildcard_dependencies() {
    let root = workspace_root();
    let ws = workspace_toml();
    let mut bad = Vec::new();

    // Check workspace-level dependencies
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
    assert!(bad.is_empty(), "Wildcard dependencies found: {bad:?}");
}

// ── 22. Workspace license field ─────────────────────────────────────────

#[test]
fn workspace_license_is_dual() {
    let ws = workspace_toml();
    let license = ws["workspace"]["package"]["license"]
        .as_str()
        .expect("workspace.package.license");
    assert_eq!(
        license, "MIT OR Apache-2.0",
        "Workspace license must be 'MIT OR Apache-2.0'"
    );
}

// ── 23. Root package is not publishable ─────────────────────────────────

#[test]
fn root_package_not_publishable() {
    let ws = workspace_toml();
    let publish = ws["package"]
        .get("publish")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    assert!(
        !publish,
        "Root workspace package should have publish = false"
    );
}

// ── 24. All crates have readme ──────────────────────────────────────────

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
        "Crates missing readme field or README.md: {missing:?}"
    );
}

// ── 25. No circular dependencies ────────────────────────────────────────

#[test]
fn no_circular_dependencies() {
    let root = workspace_root();

    // Build name → [dependency names] graph for workspace crates only
    let mut names: HashSet<String> = HashSet::new();
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();
        names.insert(name.clone());
        graph.entry(name).or_default();
    }

    for dir in workspace_member_dirs() {
        let ct = read_toml(&root.join(&dir).join("Cargo.toml"));
        let name = ct["package"]["name"].as_str().unwrap_or("?").to_string();

        // dev-dependencies may form cycles legitimately
        for section in ["dependencies", "build-dependencies"] {
            if let Some(deps) = ct.get(section).and_then(|v| v.as_table()) {
                for dep_name in deps.keys() {
                    if names.contains(dep_name) {
                        graph
                            .entry(name.clone())
                            .or_default()
                            .push(dep_name.clone());
                    }
                }
            }
        }
    }

    // BFS cycle detection for each node
    let mut cycles = Vec::new();
    for start in &names {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        if let Some(deps) = graph.get(start) {
            for d in deps {
                queue.push_back((d.clone(), vec![start.clone(), d.clone()]));
            }
        }
        while let Some((node, path)) = queue.pop_front() {
            if &node == start {
                cycles.push(path);
                break;
            }
            if !visited.insert(node.clone()) {
                continue;
            }
            if let Some(deps) = graph.get(&node) {
                for d in deps {
                    let mut p = path.clone();
                    p.push(d.clone());
                    queue.push_back((d.clone(), p));
                }
            }
        }
    }
    assert!(
        cycles.is_empty(),
        "Circular dependencies detected: {cycles:?}"
    );
}

// ── 26. Expected workspace members ──────────────────────────────────────

#[test]
fn expected_crates_are_workspace_members() {
    let members = workspace_member_dirs();
    let member_names: Vec<String> = {
        let root = workspace_root();
        members
            .iter()
            .map(|dir| {
                let ct = read_toml(&root.join(dir).join("Cargo.toml"));
                ct["package"]["name"].as_str().unwrap_or("?").to_string()
            })
            .collect()
    };
    let member_set: HashSet<&str> = member_names.iter().map(|s| s.as_str()).collect();

    let expected = [
        // core layer
        "abp-core",
        "abp-protocol",
        "abp-host",
        "abp-glob",
        "abp-workspace",
        "abp-policy",
        // backend layer
        "abp-backend-core",
        "abp-backend-mock",
        "abp-backend-sidecar",
        // integration / runtime
        "abp-integrations",
        "abp-runtime",
        "abp-cli",
        "abp-daemon",
        // SDK shims
        "abp-openai-sdk",
        "abp-claude-sdk",
        "abp-gemini-sdk",
        "abp-codex-sdk",
        "abp-kimi-sdk",
        "abp-copilot-sdk",
        // new crates
        "abp-emulation",
        "abp-telemetry",
        "abp-dialect",
        // sidecar / bridge
        "abp-sidecar-sdk",
        "abp-git",
        "sidecar-kit",
        "claude-bridge",
    ];

    let mut missing: Vec<&str> = expected
        .iter()
        .filter(|name| !member_set.contains(**name))
        .copied()
        .collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "Expected crates missing from workspace members: {missing:?}"
    );
}
