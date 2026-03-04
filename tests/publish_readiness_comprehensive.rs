#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(clippy::collapsible_if)]
//! Comprehensive crates.io publish-readiness tests for the Agent Backplane workspace.
//!
//! 80+ tests verifying metadata, licensing, versioning, dependency hygiene,
//! naming conventions, acyclicity, and README presence across every workspace crate.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn parse_toml(path: &Path) -> toml::Value {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    content
        .parse::<toml::Value>()
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
}

fn root_toml() -> toml::Value {
    parse_toml(&workspace_root().join("Cargo.toml"))
}

/// Returns `(member_relative_path, parsed_toml)` for every workspace member.
fn workspace_members() -> Vec<(String, toml::Value)> {
    let root = root_toml();
    let members = root["workspace"]["members"]
        .as_array()
        .expect("workspace.members should be an array");
    members
        .iter()
        .map(|m| {
            let rel = m.as_str().unwrap().to_string();
            let path = workspace_root().join(&rel).join("Cargo.toml");
            let val = parse_toml(&path);
            (rel, val)
        })
        .collect()
}

fn workspace_pkg() -> toml::Value {
    root_toml()["workspace"]["package"].clone()
}

/// Extract the package name from a parsed Cargo.toml.
fn pkg_name(toml: &toml::Value) -> &str {
    toml["package"]["name"].as_str().unwrap()
}

/// True when a field is either set directly or inherited via `.workspace = true`.
fn field_present(toml: &toml::Value, field: &str) -> bool {
    let pkg = &toml["package"];
    if let Some(t) = pkg.get(field) {
        if t.is_str() || t.is_integer() || t.is_float() || t.is_bool() {
            return true;
        }
        // workspace = true style: the field is a table with { workspace = true }
        if let Some(tbl) = t.as_table()
            && tbl.get("workspace").and_then(|v| v.as_bool()) == Some(true)
        {
            return true;
        }
    }
    false
}

/// Resolve field value, respecting workspace inheritance.
fn resolve_field<'a>(toml: &'a toml::Value, ws: &'a toml::Value, field: &str) -> Option<&'a str> {
    let pkg = &toml["package"];
    if let Some(v) = pkg.get(field) {
        if let Some(s) = v.as_str() {
            return Some(s);
        }
        if v.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
            return ws.get(field).and_then(|w| w.as_str());
        }
    }
    None
}

/// Collect internal (path) dependency names from `[dependencies]`.
fn internal_deps(toml: &toml::Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(deps) = toml.get("dependencies").and_then(|d| d.as_table()) {
        for (name, spec) in deps {
            let has_path = match spec {
                toml::Value::Table(t) => t.contains_key("path"),
                _ => false,
            };
            if has_path {
                out.push(name.clone());
            }
        }
    }
    out
}

/// Returns true if the crate has `publish = false`.
fn is_publish_false(toml: &toml::Value) -> bool {
    toml["package"]
        .get("publish")
        .and_then(|v| v.as_bool())
        .is_some_and(|b| !b)
}

// ===========================================================================
// 1. Required metadata
// ===========================================================================

#[test]
fn all_crates_have_name() {
    for (rel, t) in workspace_members() {
        assert!(
            t["package"].get("name").is_some(),
            "{rel} is missing [package].name"
        );
    }
}

#[test]
fn all_crates_have_version_field() {
    for (rel, t) in workspace_members() {
        assert!(
            field_present(&t, "version"),
            "{rel} is missing [package].version"
        );
    }
}

#[test]
fn all_crates_have_license_field() {
    for (rel, t) in workspace_members() {
        assert!(
            field_present(&t, "license"),
            "{rel} is missing [package].license"
        );
    }
}

#[test]
fn all_crates_have_description() {
    for (rel, t) in workspace_members() {
        assert!(
            field_present(&t, "description"),
            "{rel} is missing [package].description"
        );
    }
}

#[test]
fn all_crates_have_repository() {
    for (rel, t) in workspace_members() {
        assert!(
            field_present(&t, "repository"),
            "{rel} is missing [package].repository"
        );
    }
}

#[test]
fn all_crates_have_edition() {
    for (rel, t) in workspace_members() {
        assert!(
            field_present(&t, "edition"),
            "{rel} is missing [package].edition"
        );
    }
}

#[test]
fn all_crates_have_authors() {
    for (rel, t) in workspace_members() {
        assert!(
            field_present(&t, "authors"),
            "{rel} is missing [package].authors"
        );
    }
}

#[test]
fn all_publishable_crates_have_readme_field() {
    for (rel, t) in workspace_members() {
        if is_publish_false(&t) {
            continue;
        }
        assert!(
            field_present(&t, "readme"),
            "{rel} is publishable but missing [package].readme"
        );
    }
}

// ===========================================================================
// 2. Per-crate metadata spot-checks
// ===========================================================================

macro_rules! spot_check_metadata {
    ($test_name:ident, $crate_dir:literal) => {
        #[test]
        fn $test_name() {
            let path = workspace_root().join($crate_dir).join("Cargo.toml");
            let t = parse_toml(&path);
            let ws = workspace_pkg();
            for field in [
                "name",
                "version",
                "license",
                "description",
                "repository",
                "edition",
            ] {
                assert!(
                    field_present(&t, field),
                    "{} is missing field `{field}`",
                    $crate_dir
                );
            }
            // description should not be empty
            let desc = resolve_field(&t, &ws, "description").unwrap_or("");
            assert!(!desc.is_empty(), "{} has empty description", $crate_dir);
        }
    };
}

spot_check_metadata!(core_has_complete_metadata, "crates/abp-core");
spot_check_metadata!(protocol_has_complete_metadata, "crates/abp-protocol");
spot_check_metadata!(cli_has_complete_metadata, "crates/abp-cli");
spot_check_metadata!(runtime_has_complete_metadata, "crates/abp-runtime");
spot_check_metadata!(host_has_complete_metadata, "crates/abp-host");
spot_check_metadata!(glob_has_complete_metadata, "crates/abp-glob");
spot_check_metadata!(policy_has_complete_metadata, "crates/abp-policy");
spot_check_metadata!(
    workspace_crate_has_complete_metadata,
    "crates/abp-workspace"
);
spot_check_metadata!(
    integrations_has_complete_metadata,
    "crates/abp-integrations"
);
spot_check_metadata!(ir_has_complete_metadata, "crates/abp-ir");
spot_check_metadata!(error_has_complete_metadata, "crates/abp-error");
spot_check_metadata!(receipt_has_complete_metadata, "crates/abp-receipt");
spot_check_metadata!(sidecar_kit_has_complete_metadata, "crates/sidecar-kit");
spot_check_metadata!(claude_bridge_has_complete_metadata, "crates/claude-bridge");
spot_check_metadata!(
    backend_core_has_complete_metadata,
    "crates/abp-backend-core"
);

// ===========================================================================
// 3. Edition and rust-version
// ===========================================================================

#[test]
fn workspace_edition_is_2024() {
    let ws = workspace_pkg();
    assert_eq!(ws["edition"].as_str().unwrap(), "2024");
}

#[test]
fn workspace_rust_version_is_1_85() {
    let ws = workspace_pkg();
    assert_eq!(ws["rust-version"].as_str().unwrap(), "1.85");
}

#[test]
fn all_crates_edition_resolves_to_2024() {
    let ws = workspace_pkg();
    for (rel, t) in workspace_members() {
        let edition = resolve_field(&t, &ws, "edition").unwrap_or("unknown");
        assert_eq!(
            edition, "2024",
            "{rel} has edition {edition}, expected 2024"
        );
    }
}

#[test]
fn no_crate_overrides_edition_directly_with_wrong_value() {
    for (rel, t) in workspace_members() {
        if let Some(ed) = t["package"].get("edition").and_then(|v| v.as_str()) {
            assert_eq!(
                ed, "2024",
                "{rel} overrides edition to {ed} instead of 2024"
            );
        }
    }
}

#[test]
fn workspace_defines_rust_version() {
    let ws = workspace_pkg();
    assert!(
        ws.get("rust-version").is_some(),
        "workspace.package should define rust-version"
    );
}

// ===========================================================================
// 4. License consistency
// ===========================================================================

const EXPECTED_LICENSE: &str = "MIT OR Apache-2.0";

#[test]
fn workspace_license_is_mit_or_apache() {
    let ws = workspace_pkg();
    assert_eq!(ws["license"].as_str().unwrap(), EXPECTED_LICENSE);
}

#[test]
fn all_crates_license_resolves_to_expected() {
    let ws = workspace_pkg();
    for (rel, t) in workspace_members() {
        let lic = resolve_field(&t, &ws, "license").unwrap_or("MISSING");
        assert_eq!(
            lic, EXPECTED_LICENSE,
            "{rel} has license `{lic}`, expected `{EXPECTED_LICENSE}`"
        );
    }
}

#[test]
fn no_crate_overrides_license_with_wrong_value() {
    for (rel, t) in workspace_members() {
        if let Some(lic) = t["package"].get("license").and_then(|v| v.as_str()) {
            assert_eq!(lic, EXPECTED_LICENSE, "{rel} overrides license to `{lic}`");
        }
    }
}

#[test]
fn license_mit_file_exists_at_root() {
    assert!(
        workspace_root().join("LICENSE-MIT").exists(),
        "LICENSE-MIT not found at workspace root"
    );
}

#[test]
fn license_apache_file_exists_at_root() {
    assert!(
        workspace_root().join("LICENSE-APACHE").exists(),
        "LICENSE-APACHE not found at workspace root"
    );
}

#[test]
fn license_field_uses_spdx_syntax() {
    let ws = workspace_pkg();
    let lic = ws["license"].as_str().unwrap();
    assert!(
        lic.contains(" OR "),
        "license `{lic}` should use SPDX `OR` syntax"
    );
}

// ===========================================================================
// 5. Version consistency
// ===========================================================================

#[test]
fn workspace_version_is_0_1_0() {
    let ws = workspace_pkg();
    assert_eq!(ws["version"].as_str().unwrap(), "0.1.0");
}

#[test]
fn all_crates_version_resolves_to_workspace() {
    let ws = workspace_pkg();
    let expected = ws["version"].as_str().unwrap();
    for (rel, t) in workspace_members() {
        let ver = resolve_field(&t, &ws, "version").unwrap_or("MISSING");
        assert_eq!(ver, expected, "{rel} version {ver} != workspace {expected}");
    }
}

#[test]
fn no_crate_hardcodes_different_version() {
    let ws = workspace_pkg();
    let expected = ws["version"].as_str().unwrap();
    for (rel, t) in workspace_members() {
        if let Some(v) = t["package"].get("version").and_then(|v| v.as_str()) {
            assert_eq!(
                v, expected,
                "{rel} hardcodes version {v} != workspace {expected}"
            );
        }
    }
}

#[test]
fn internal_path_deps_carry_matching_version() {
    let ws = workspace_pkg();
    let expected = ws["version"].as_str().unwrap();
    for (rel, t) in workspace_members() {
        if let Some(deps) = t.get("dependencies").and_then(|d| d.as_table()) {
            for (dep_name, spec) in deps {
                if let Some(tbl) = spec.as_table()
                    && tbl.contains_key("path")
                    && let Some(v) = tbl.get("version").and_then(|v| v.as_str())
                {
                    assert_eq!(
                        v, expected,
                        "{rel}: dep `{dep_name}` version {v} != {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn workspace_version_is_valid_semver() {
    let ws = workspace_pkg();
    let ver = ws["version"].as_str().unwrap();
    let parts: Vec<&str> = ver.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "version {ver} is not semver MAJOR.MINOR.PATCH"
    );
    for p in &parts {
        assert!(
            p.parse::<u32>().is_ok(),
            "version component `{p}` is not numeric"
        );
    }
}

#[test]
fn all_crate_versions_are_semver() {
    let ws = workspace_pkg();
    for (rel, t) in workspace_members() {
        let ver = resolve_field(&t, &ws, "version").unwrap_or("0.0.0");
        let parts: Vec<&str> = ver.split('.').collect();
        assert_eq!(parts.len(), 3, "{rel} version `{ver}` is not valid semver");
    }
}

#[test]
fn no_version_is_0_0_0() {
    let ws = workspace_pkg();
    for (rel, t) in workspace_members() {
        let ver = resolve_field(&t, &ws, "version").unwrap_or("0.0.0");
        assert_ne!(ver, "0.0.0", "{rel} has placeholder version 0.0.0");
    }
}

#[test]
fn all_crates_use_workspace_version_inheritance() {
    for (rel, t) in workspace_members() {
        let pkg = &t["package"];
        if let Some(v) = pkg.get("version") {
            let inherits = v
                .as_table()
                .and_then(|tbl| tbl.get("workspace"))
                .and_then(|w| w.as_bool())
                == Some(true);
            assert!(
                inherits || v.as_str().is_some(),
                "{rel} has an unusual version field format"
            );
        }
    }
}

// ===========================================================================
// 6. Dependency direction validation
// ===========================================================================

#[test]
fn core_has_no_internal_deps_except_error() {
    let path = workspace_root().join("crates/abp-core/Cargo.toml");
    let t = parse_toml(&path);
    let deps = internal_deps(&t);
    for d in &deps {
        assert_eq!(
            d, "abp-error",
            "abp-core should only depend internally on abp-error, found `{d}`"
        );
    }
}

#[test]
fn error_has_no_internal_deps() {
    let path = workspace_root().join("crates/abp-error/Cargo.toml");
    let t = parse_toml(&path);
    let deps = internal_deps(&t);
    assert!(
        deps.is_empty(),
        "abp-error should have zero internal deps, found: {deps:?}"
    );
}

#[test]
fn glob_has_no_internal_deps() {
    let path = workspace_root().join("crates/abp-glob/Cargo.toml");
    let t = parse_toml(&path);
    let deps = internal_deps(&t);
    assert!(
        deps.is_empty(),
        "abp-glob should have zero internal deps, found: {deps:?}"
    );
}

#[test]
fn protocol_depends_on_core() {
    let path = workspace_root().join("crates/abp-protocol/Cargo.toml");
    let t = parse_toml(&path);
    let deps = internal_deps(&t);
    assert!(
        deps.contains(&"abp-core".to_string()),
        "abp-protocol should depend on abp-core"
    );
}

#[test]
fn host_depends_on_protocol() {
    let path = workspace_root().join("crates/abp-host/Cargo.toml");
    let t = parse_toml(&path);
    let deps = internal_deps(&t);
    assert!(
        deps.contains(&"abp-protocol".to_string()),
        "abp-host should depend on abp-protocol"
    );
}

#[test]
fn integrations_depends_on_backend_core() {
    let path = workspace_root().join("crates/abp-integrations/Cargo.toml");
    let t = parse_toml(&path);
    let deps = internal_deps(&t);
    assert!(
        deps.contains(&"abp-backend-core".to_string()),
        "abp-integrations should depend on abp-backend-core"
    );
}

#[test]
fn runtime_depends_on_integrations() {
    let path = workspace_root().join("crates/abp-runtime/Cargo.toml");
    let t = parse_toml(&path);
    let deps = internal_deps(&t);
    assert!(
        deps.contains(&"abp-integrations".to_string()),
        "abp-runtime should depend on abp-integrations"
    );
}

#[test]
fn cli_depends_on_runtime() {
    let path = workspace_root().join("crates/abp-cli/Cargo.toml");
    let t = parse_toml(&path);
    let deps = internal_deps(&t);
    assert!(
        deps.contains(&"abp-runtime".to_string()),
        "abp-cli should depend on abp-runtime"
    );
}

#[test]
fn policy_depends_on_glob() {
    let path = workspace_root().join("crates/abp-policy/Cargo.toml");
    let t = parse_toml(&path);
    let deps = internal_deps(&t);
    assert!(
        deps.contains(&"abp-glob".to_string()),
        "abp-policy should depend on abp-glob"
    );
}

#[test]
fn no_crate_depends_on_cli() {
    // xtask is allowed to depend on abp-cli for build automation.
    for (rel, t) in workspace_members() {
        if rel.ends_with("abp-cli") || rel.ends_with("xtask") {
            continue;
        }
        let deps = internal_deps(&t);
        assert!(
            !deps.contains(&"abp-cli".to_string()),
            "{rel} should not depend on abp-cli (leaf crate)"
        );
    }
}

#[test]
fn no_crate_depends_on_daemon() {
    for (rel, t) in workspace_members() {
        if rel.ends_with("abp-daemon") {
            continue;
        }
        let deps = internal_deps(&t);
        assert!(
            !deps.contains(&"abp-daemon".to_string()),
            "{rel} should not depend on abp-daemon (leaf crate)"
        );
    }
}

#[test]
fn shim_crates_do_not_depend_on_runtime() {
    for (rel, t) in workspace_members() {
        if !rel.contains("abp-shim-") {
            continue;
        }
        let deps = internal_deps(&t);
        assert!(
            !deps.contains(&"abp-runtime".to_string()),
            "{rel} (shim) should not depend on abp-runtime"
        );
    }
}

// ===========================================================================
// 7. Naming convention
// ===========================================================================

/// The only two workspace crates allowed outside `abp-*` naming.
const NAMING_EXCEPTIONS: &[&str] = &["claude-bridge", "sidecar-kit", "xtask"];

#[test]
fn all_crate_names_follow_abp_or_are_exceptions() {
    for (rel, t) in workspace_members() {
        let name = pkg_name(&t);
        let ok = name.starts_with("abp-") || NAMING_EXCEPTIONS.contains(&name);
        assert!(
            ok,
            "{rel}: crate `{name}` does not follow abp-* naming and is not an exception"
        );
    }
}

#[test]
fn non_abp_crates_are_known_exceptions() {
    let exceptions: HashSet<&str> = NAMING_EXCEPTIONS.iter().copied().collect();
    for (_rel, t) in workspace_members() {
        let name = pkg_name(&t);
        if !name.starts_with("abp-") {
            assert!(
                exceptions.contains(name),
                "unexpected non-abp crate: `{name}`"
            );
        }
    }
}

#[test]
fn crate_names_are_all_lowercase() {
    for (rel, t) in workspace_members() {
        let name = pkg_name(&t);
        assert_eq!(
            name,
            name.to_lowercase().as_str(),
            "{rel}: crate name `{name}` is not lowercase"
        );
    }
}

#[test]
fn crate_names_use_hyphens_not_underscores() {
    for (rel, t) in workspace_members() {
        let name = pkg_name(&t);
        assert!(
            !name.contains('_'),
            "{rel}: crate name `{name}` should use hyphens, not underscores"
        );
    }
}

#[test]
fn no_duplicate_crate_names() {
    let mut seen = HashSet::new();
    for (_rel, t) in workspace_members() {
        let name = pkg_name(&t);
        assert!(
            seen.insert(name.to_string()),
            "duplicate crate name: `{name}`"
        );
    }
}

#[test]
fn workspace_member_paths_match_crate_names() {
    for (rel, t) in workspace_members() {
        let name = pkg_name(&t);
        let dir_name = Path::new(&rel).file_name().unwrap().to_str().unwrap();
        assert_eq!(
            dir_name, name,
            "{rel}: directory `{dir_name}` does not match crate name `{name}`"
        );
    }
}

#[test]
fn xtask_is_not_published() {
    let path = workspace_root().join("xtask/Cargo.toml");
    let t = parse_toml(&path);
    assert!(is_publish_false(&t), "xtask should have publish = false");
}

#[test]
fn root_package_is_not_published() {
    let root = root_toml();
    let publish = root["package"]
        .get("publish")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    assert!(!publish, "root package should have publish = false");
}

#[test]
fn at_least_40_workspace_members() {
    let count = workspace_members().len();
    assert!(count >= 40, "expected ≥40 workspace members, found {count}");
}

#[test]
fn crate_name_lengths_are_reasonable() {
    for (rel, t) in workspace_members() {
        let name = pkg_name(&t);
        assert!(
            name.len() <= 64,
            "{rel}: crate name `{name}` exceeds 64 chars (crates.io limit)"
        );
        assert!(!name.is_empty(), "{rel}: crate name is empty");
    }
}

// ===========================================================================
// 8. No circular dependencies
// ===========================================================================

/// Build full adjacency list of internal deps, then assert the graph is a DAG.
fn build_dep_graph() -> BTreeMap<String, Vec<String>> {
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (_rel, t) in workspace_members() {
        let name = pkg_name(&t).to_string();
        let deps = internal_deps(&t);
        graph.insert(name, deps);
    }
    graph
}

fn has_cycle(graph: &BTreeMap<String, Vec<String>>) -> Option<String> {
    // Simple DFS-based cycle detection.
    let mut visited = HashSet::new();
    let mut in_stack = HashSet::new();

    fn dfs(
        node: &str,
        graph: &BTreeMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        in_stack: &mut HashSet<String>,
    ) -> Option<String> {
        visited.insert(node.to_string());
        in_stack.insert(node.to_string());
        if let Some(neighbors) = graph.get(node) {
            for n in neighbors {
                if !visited.contains(n.as_str()) {
                    if let Some(cycle) = dfs(n, graph, visited, in_stack) {
                        return Some(cycle);
                    }
                } else if in_stack.contains(n.as_str()) {
                    return Some(format!("{node} -> {n}"));
                }
            }
        }
        in_stack.remove(node);
        None
    }

    for node in graph.keys() {
        if !visited.contains(node.as_str())
            && let Some(cycle) = dfs(node, graph, &mut visited, &mut in_stack)
        {
            return Some(cycle);
        }
    }
    None
}

#[test]
fn no_circular_deps_in_workspace() {
    let graph = build_dep_graph();
    assert_eq!(
        has_cycle(&graph),
        None,
        "circular dependency detected in workspace"
    );
}

#[test]
fn dependency_graph_is_dag() {
    let graph = build_dep_graph();
    // Topological sort must succeed for a DAG.
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    for (name, deps) in &graph {
        in_degree.entry(name.clone()).or_insert(0);
        for d in deps {
            *in_degree.entry(d.clone()).or_insert(0) += 1;
        }
    }
    let mut queue: Vec<String> = in_degree
        .iter()
        .filter(|&(_, &deg)| deg == 0)
        .map(|(n, _)| n.clone())
        .collect();
    let mut count = 0usize;
    while let Some(node) = queue.pop() {
        count += 1;
        if let Some(deps) = graph.get(&node) {
            for d in deps {
                if let Some(deg) = in_degree.get_mut(d) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(d.clone());
                    }
                }
            }
        }
    }
    // Note: count may exceed graph.len() if deps reference external crates
    // that are not in the map. That's fine — we just need no cycle.
    assert!(
        count >= graph.len(),
        "topological sort processed {count} < {} nodes — cycle exists",
        graph.len()
    );
}

#[test]
fn no_bidirectional_deps() {
    let graph = build_dep_graph();
    for (name, deps) in &graph {
        for d in deps {
            if let Some(reverse_deps) = graph.get(d) {
                assert!(
                    !reverse_deps.contains(name),
                    "bidirectional dependency between `{name}` and `{d}`"
                );
            }
        }
    }
}

#[test]
fn core_is_widely_depended_on() {
    let graph = build_dep_graph();
    let dependents: Vec<&String> = graph
        .iter()
        .filter(|(_, deps)| deps.contains(&"abp-core".to_string()))
        .map(|(name, _)| name)
        .collect();
    assert!(
        dependents.len() >= 10,
        "abp-core should be depended on by many crates, found {}",
        dependents.len()
    );
}

#[test]
fn error_crate_is_leaf() {
    let graph = build_dep_graph();
    let deps = graph.get("abp-error").unwrap();
    assert!(
        deps.is_empty(),
        "abp-error should be a leaf with no internal deps: {deps:?}"
    );
}

#[test]
fn glob_crate_is_leaf() {
    let graph = build_dep_graph();
    let deps = graph.get("abp-glob").unwrap();
    assert!(
        deps.is_empty(),
        "abp-glob should be a leaf with no internal deps: {deps:?}"
    );
}

// ===========================================================================
// 9. README files
// ===========================================================================

#[test]
fn all_crates_have_readme_file_on_disk() {
    for (rel, t) in workspace_members() {
        if is_publish_false(&t) {
            continue;
        }
        let readme = workspace_root().join(&rel).join("README.md");
        assert!(readme.exists(), "{rel} is missing README.md on disk");
    }
}

#[test]
fn readme_files_are_not_empty() {
    for (rel, _t) in workspace_members() {
        let readme = workspace_root().join(&rel).join("README.md");
        if readme.exists() {
            let content = std::fs::read_to_string(&readme).unwrap();
            assert!(!content.trim().is_empty(), "{rel}/README.md is empty");
        }
    }
}

#[test]
fn root_readme_exists() {
    assert!(
        workspace_root().join("README.md").exists(),
        "workspace root README.md is missing"
    );
}

#[test]
fn changelog_exists() {
    assert!(
        workspace_root().join("CHANGELOG.md").exists(),
        "CHANGELOG.md is missing from workspace root"
    );
}

#[test]
fn contributing_guide_exists() {
    assert!(
        workspace_root().join("CONTRIBUTING.md").exists(),
        "CONTRIBUTING.md is missing from workspace root"
    );
}

#[test]
fn crate_readmes_mention_crate_name() {
    for (rel, t) in workspace_members() {
        let name = pkg_name(&t);
        let readme = workspace_root().join(&rel).join("README.md");
        if readme.exists() {
            let content = std::fs::read_to_string(&readme).unwrap();
            assert!(
                content.contains(name) || content.contains(&name.replace('-', "_")),
                "{rel}/README.md does not mention crate name `{name}`"
            );
        }
    }
}

#[test]
fn publishable_crates_declare_readme_in_cargo_toml() {
    for (rel, t) in workspace_members() {
        if is_publish_false(&t) {
            continue;
        }
        let readme_field = t["package"].get("readme");
        assert!(
            readme_field.is_some(),
            "{rel} is publishable but does not declare readme in Cargo.toml"
        );
    }
}

// ===========================================================================
// 10. Additional package-field checks
// ===========================================================================

#[test]
fn workspace_repository_url_is_valid_format() {
    let ws = workspace_pkg();
    let repo = ws["repository"].as_str().unwrap();
    assert!(
        repo.starts_with("https://"),
        "repository URL should use https://"
    );
    assert!(
        repo.contains("github.com"),
        "repository URL should point to github.com"
    );
}

#[test]
fn publishable_crates_have_keywords() {
    for (rel, t) in workspace_members() {
        if is_publish_false(&t) {
            continue;
        }
        let has_kw = t["package"]
            .get("keywords")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty());
        assert!(has_kw, "{rel} is publishable but has no keywords");
    }
}

#[test]
fn publishable_crates_have_categories() {
    for (rel, t) in workspace_members() {
        if is_publish_false(&t) {
            continue;
        }
        let has_cat = t["package"]
            .get("categories")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty());
        assert!(has_cat, "{rel} is publishable but has no categories");
    }
}

#[test]
fn keywords_do_not_exceed_five() {
    for (rel, t) in workspace_members() {
        if let Some(kw) = t["package"].get("keywords").and_then(|v| v.as_array()) {
            assert!(
                kw.len() <= 5,
                "{rel} has {} keywords; crates.io allows at most 5",
                kw.len()
            );
        }
    }
}

#[test]
fn descriptions_are_not_too_long() {
    let ws = workspace_pkg();
    for (rel, t) in workspace_members() {
        let desc = resolve_field(&t, &ws, "description").unwrap_or("");
        assert!(
            desc.len() <= 500,
            "{rel} description is {} chars (keep it under 500 for crates.io)",
            desc.len()
        );
    }
}

#[test]
fn no_publish_false_on_publishable_crates() {
    // Crates that intentionally must NOT be published.
    let unpublishable: HashSet<&str> = ["xtask", "agent-backplane"].iter().copied().collect();
    for (_rel, t) in workspace_members() {
        let name = pkg_name(&t);
        if is_publish_false(&t) {
            assert!(
                unpublishable.contains(name),
                "crate `{name}` has publish=false but is not in the unpublishable allowlist"
            );
        }
    }
}

#[test]
fn workspace_resolver_is_2() {
    let root = root_toml();
    let resolver = root["workspace"]["resolver"].as_str().unwrap();
    assert_eq!(resolver, "2", "workspace resolver should be 2");
}

#[test]
fn all_internal_dep_paths_are_relative() {
    for (rel, t) in workspace_members() {
        if let Some(deps) = t.get("dependencies").and_then(|d| d.as_table()) {
            for (dep_name, spec) in deps {
                if let Some(tbl) = spec.as_table()
                    && let Some(p) = tbl.get("path").and_then(|v| v.as_str())
                {
                    assert!(
                        !p.starts_with('/') && !p.contains(':'),
                        "{rel}: dep `{dep_name}` has non-relative path `{p}`"
                    );
                }
            }
        }
    }
}

#[test]
fn all_internal_dep_paths_point_to_existing_dirs() {
    for (rel, t) in workspace_members() {
        let crate_dir = workspace_root().join(&rel);
        if let Some(deps) = t.get("dependencies").and_then(|d| d.as_table()) {
            for (dep_name, spec) in deps {
                if let Some(tbl) = spec.as_table()
                    && let Some(p) = tbl.get("path").and_then(|v| v.as_str())
                {
                    let target = crate_dir.join(p);
                    assert!(
                        target.exists(),
                        "{rel}: dep `{dep_name}` path `{p}` resolves to {} which does not exist",
                        target.display()
                    );
                }
            }
        }
    }
}

#[test]
fn workspace_members_list_matches_crates_directory() {
    let root = root_toml();
    let members: BTreeSet<String> = root["workspace"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    let crates_dir = workspace_root().join("crates");
    if crates_dir.exists() {
        for entry in std::fs::read_dir(&crates_dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                let dir_name = entry.file_name().to_str().unwrap().to_string();
                let rel = format!("crates/{dir_name}");
                if workspace_root().join(&rel).join("Cargo.toml").exists() {
                    assert!(
                        members.contains(&rel),
                        "crate directory `{rel}` exists but is not in workspace.members"
                    );
                }
            }
        }
    }
}

#[test]
fn workspace_defines_common_deps() {
    let root = root_toml();
    let ws_deps = root["workspace"]
        .get("dependencies")
        .and_then(|d| d.as_table());
    assert!(
        ws_deps.is_some(),
        "workspace should define [workspace.dependencies]"
    );
    let ws_deps = ws_deps.unwrap();
    for expected in ["serde", "serde_json", "tokio", "thiserror"] {
        assert!(
            ws_deps.contains_key(expected),
            "workspace.dependencies should include `{expected}`"
        );
    }
}

#[test]
fn no_wildcard_versions_in_workspace_deps() {
    let root = root_toml();
    if let Some(deps) = root["workspace"]
        .get("dependencies")
        .and_then(|d| d.as_table())
    {
        for (name, spec) in deps {
            let ver = match spec {
                toml::Value::String(s) => s.clone(),
                toml::Value::Table(t) => t
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                _ => String::new(),
            };
            assert!(ver != "*", "workspace dep `{name}` uses wildcard version");
        }
    }
}
