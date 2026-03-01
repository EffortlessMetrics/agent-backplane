//! CI hardening tests that enforce workspace-wide conventions.
//!
//! These tests verify structural properties across all workspace crates:
//! metadata completeness, code hygiene, dependency health, and trait coverage.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Workspace root directory (parent of `tests/`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Parse root Cargo.toml and return workspace member relative paths.
fn workspace_members() -> Vec<String> {
    let root = workspace_root();
    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");
    let doc: toml::Value = cargo_toml.parse().expect("parse root Cargo.toml");

    doc["workspace"]["members"]
        .as_array()
        .expect("workspace.members array")
        .iter()
        .map(|v| v.as_str().expect("member string").to_string())
        .collect()
}

/// Parse a crate's Cargo.toml and return the toml::Value.
fn crate_manifest(member: &str) -> toml::Value {
    let path = workspace_root().join(member).join("Cargo.toml");
    let content =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    content
        .parse()
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Recursively collect all `.rs` files under a directory.
fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                result.extend(collect_rs_files(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                result.push(path);
            }
        }
    }
    result
}

/// Collect all `.rs` source files under `src/` for a given workspace member.
fn source_files(member: &str) -> Vec<PathBuf> {
    let src_dir = workspace_root().join(member).join("src");
    if !src_dir.exists() {
        return Vec::new();
    }
    collect_rs_files(&src_dir)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn verify_workspace_members_exist() {
    let root = workspace_root();
    let mut missing = Vec::new();
    for member in workspace_members() {
        if !root.join(&member).join("Cargo.toml").exists() {
            missing.push(member);
        }
    }
    assert!(
        missing.is_empty(),
        "workspace members with missing Cargo.toml: {missing:?}"
    );
}

#[test]
fn verify_all_crates_have_license_field() {
    let mut bad = Vec::new();
    for member in workspace_members() {
        let manifest = crate_manifest(&member);
        let pkg = &manifest["package"];
        if pkg.get("license").is_none() {
            bad.push(member);
        }
    }
    assert!(bad.is_empty(), "crates missing license field: {bad:?}");
}

#[test]
fn verify_all_crates_have_description_field() {
    let mut bad = Vec::new();
    for member in workspace_members() {
        let manifest = crate_manifest(&member);
        let pkg = &manifest["package"];
        if pkg.get("description").is_none() {
            bad.push(member);
        }
    }
    assert!(bad.is_empty(), "crates missing description field: {bad:?}");
}

#[test]
fn verify_all_crates_version_matches_workspace() {
    let root_toml: toml::Value = fs::read_to_string(workspace_root().join("Cargo.toml"))
        .expect("read root Cargo.toml")
        .parse()
        .expect("parse root Cargo.toml");
    let ws_version = root_toml["workspace"]["package"]["version"]
        .as_str()
        .expect("workspace version string");

    let mut bad = Vec::new();
    for member in workspace_members() {
        let manifest = crate_manifest(&member);
        let pkg = &manifest["package"];
        match pkg.get("version") {
            // version.workspace = true  →  inherited from workspace
            Some(toml::Value::Table(t))
                if t.get("workspace").and_then(|v| v.as_bool()) == Some(true) => {}
            // Explicit version must match workspace
            Some(toml::Value::String(s)) if s == ws_version => {}
            Some(toml::Value::String(s)) => {
                bad.push(format!("{member} (version {s} != workspace {ws_version})"));
            }
            _ => {
                bad.push(format!("{member} (no version field)"));
            }
        }
    }
    assert!(bad.is_empty(), "crates with version mismatch: {bad:?}");
}

#[test]
fn verify_all_crates_have_edition() {
    let mut bad = Vec::new();
    for member in workspace_members() {
        let manifest = crate_manifest(&member);
        if manifest["package"].get("edition").is_none() {
            bad.push(member);
        }
    }
    assert!(bad.is_empty(), "crates missing edition field: {bad:?}");
}

#[test]
fn verify_all_crates_have_warn_missing_docs() {
    let root = workspace_root();
    let mut bad = Vec::new();
    for member in workspace_members() {
        let lib_rs = root.join(&member).join("src").join("lib.rs");
        if !lib_rs.exists() {
            continue; // binary-only crates
        }
        let content = fs::read_to_string(&lib_rs).expect("read lib.rs");
        if !content.contains("#![warn(missing_docs)]")
            && !content.contains("#![deny(missing_docs)]")
        {
            bad.push(member);
        }
    }
    assert!(
        bad.is_empty(),
        "crates missing #![warn(missing_docs)] in lib.rs: {bad:?}"
    );
}

#[test]
fn verify_no_circular_dependencies() {
    let members = workspace_members();

    // Build crate-name → set-of-workspace-dep-names map.
    let mut name_to_member: BTreeMap<String, String> = BTreeMap::new();
    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for member in &members {
        let manifest = crate_manifest(member);
        let name = manifest["package"]["name"]
            .as_str()
            .expect("package name")
            .to_string();
        name_to_member.insert(name.clone(), member.clone());
        deps.insert(name, BTreeSet::new());
    }

    let ws_names: HashSet<&str> = name_to_member.keys().map(|s| s.as_str()).collect();

    for member in &members {
        let manifest = crate_manifest(member);
        let name = manifest["package"]["name"].as_str().unwrap().to_string();
        if let Some(dep_table) = manifest.get("dependencies").and_then(|d| d.as_table()) {
            for dep_name in dep_table.keys() {
                if ws_names.contains(dep_name.as_str()) {
                    deps.get_mut(&name).unwrap().insert(dep_name.clone());
                }
            }
        }
    }

    // Cycle detection via DFS.
    let mut visited = HashSet::new();
    let mut in_stack = HashSet::new();
    let mut cycle_found: Vec<String> = Vec::new();

    fn dfs(
        node: &str,
        deps: &BTreeMap<String, BTreeSet<String>>,
        visited: &mut HashSet<String>,
        in_stack: &mut HashSet<String>,
        path: &mut Vec<String>,
        cycle_found: &mut Vec<String>,
    ) {
        if in_stack.contains(node) {
            *cycle_found = path.clone();
            cycle_found.push(node.to_string());
            return;
        }
        if visited.contains(node) {
            return;
        }
        visited.insert(node.to_string());
        in_stack.insert(node.to_string());
        path.push(node.to_string());

        if let Some(neighbors) = deps.get(node) {
            for next in neighbors {
                if !cycle_found.is_empty() {
                    return;
                }
                dfs(next, deps, visited, in_stack, path, cycle_found);
            }
        }

        path.pop();
        in_stack.remove(node);
    }

    for name in deps.keys() {
        if cycle_found.is_empty() {
            dfs(
                name,
                &deps,
                &mut visited,
                &mut in_stack,
                &mut Vec::new(),
                &mut cycle_found,
            );
        }
    }

    assert!(
        cycle_found.is_empty(),
        "circular dependency detected: {}",
        cycle_found.join(" -> ")
    );
}

#[test]
fn verify_cargo_lock_exists_and_tracked() {
    let lock_path = workspace_root().join("Cargo.lock");
    assert!(
        lock_path.exists(),
        "Cargo.lock must exist in workspace root"
    );
    assert!(
        lock_path.metadata().map(|m| m.len() > 0).unwrap_or(false),
        "Cargo.lock must not be empty"
    );
}

/// Shared helper: assert that `marker` (e.g. "TODO") does not appear in any
/// production source comment.  Test-only files are exempt.
fn check_marker_absence(marker: &str) {
    let mut hits = Vec::new();
    for member in workspace_members() {
        for path in source_files(&member) {
            let path_str = path.to_string_lossy();
            // Skip test modules / test helper files.
            if path_str.contains("tests")
                || path_str.contains("test_")
                || path_str.ends_with("_test.rs")
            {
                continue;
            }
            let content = fs::read_to_string(&path).expect("read source file");
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                // Only flag comments containing the marker.
                if trimmed.starts_with("//") && trimmed.contains(marker) {
                    hits.push(format!("{}:{}", path.display(), i + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "{marker} found in production code:\n  {}",
        hits.join("\n  ")
    );
}

#[test]
fn verify_no_todo_in_production_code() {
    check_marker_absence("TODO");
}

#[test]
fn verify_no_fixme_in_production_code() {
    check_marker_absence("FIXME");
}

#[test]
fn verify_no_hack_in_production_code() {
    check_marker_absence("HACK");
}

#[test]
fn verify_public_types_derive_debug() {
    let mut bad = Vec::new();
    for member in workspace_members() {
        for path in source_files(&member) {
            let content = fs::read_to_string(&path).expect("read source file");
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                // Match `pub struct Foo {` or `pub enum Bar {`.
                let is_pub_type = (trimmed.starts_with("pub struct ")
                    || trimmed.starts_with("pub enum "))
                    && trimmed.ends_with('{');
                if !is_pub_type {
                    continue;
                }

                // Extract type name for manual-impl search.
                let name = trimmed
                    .split_whitespace()
                    .nth(2)
                    .unwrap_or("")
                    .split('<')
                    .next()
                    .unwrap_or("")
                    .trim();

                // Collect the full derive attribute block (may span multiple lines).
                let mut derive_text = String::new();
                let mut has_derive = false;
                for j in (0..i).rev() {
                    let prev = lines[j].trim();
                    if prev.starts_with("#[derive(") {
                        has_derive = true;
                        // Accumulate from this line forward to closing `)]`
                        for line in lines.iter().take(i).skip(j) {
                            derive_text.push_str(line.trim());
                            if line.contains(")]") {
                                break;
                            }
                        }
                        break;
                    }
                    // Stop at non-attribute, non-doc, non-empty lines.
                    if !prev.is_empty()
                        && !prev.starts_with("#[")
                        && !prev.starts_with("///")
                        && !prev.starts_with("//!")
                    {
                        break;
                    }
                }

                // Only flag types that HAVE a derive block but are missing Debug.
                // Types without any derive likely contain non-Debug fields.
                if !has_derive {
                    continue;
                }

                if derive_text.contains("Debug") {
                    continue;
                }

                // Types whose derive only includes Clone/Default/Copy are
                // typically runtime wrappers around non-Debug fields (channels,
                // atomics, dyn traits).  Don't require Debug for those.
                let derive_only_trivial = {
                    let inner = derive_text
                        .trim_start_matches("#[derive(")
                        .trim_end_matches(")]");
                    inner
                        .split(',')
                        .map(str::trim)
                        .all(|t| matches!(t, "Clone" | "Default" | "Copy" | ""))
                };
                if derive_only_trivial {
                    continue;
                }

                // Also check for manual `impl ... Debug for TypeName` in the file.
                if !name.is_empty() {
                    let pattern = format!("for {name}");
                    let manual = lines
                        .iter()
                        .any(|l| l.contains("Debug") && l.contains("impl") && l.contains(&pattern));
                    if manual {
                        continue;
                    }
                }

                bad.push(format!("{}:{}: {}", path.display(), i + 1, trimmed));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "public types with derive block missing Debug:\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn verify_error_types_implement_std_error() {
    let mut bad = Vec::new();
    for member in workspace_members() {
        for path in source_files(&member) {
            let content = fs::read_to_string(&path).expect("read source file");
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                // Only check `pub enum FooError {` — struct error types are
                // often API response DTOs rather than Rust error types.
                let is_error_enum = trimmed.starts_with("pub enum ")
                    && trimmed.contains("Error")
                    && trimmed.ends_with('{');
                if !is_error_enum {
                    continue;
                }

                // Extract type name.
                let name = trimmed
                    .split_whitespace()
                    .nth(2)
                    .unwrap_or("")
                    .trim_end_matches('{')
                    .trim();

                // Skip non-error types that happen to contain "Error".
                if name.ends_with("ErrorKind") || name.ends_with("ErrorCategory") {
                    continue;
                }
                // ErrorCode in abp-error is a stable code enum, not an error type.
                if name == "ErrorCode" {
                    continue;
                }

                // Check for thiserror derive OR manual std::error::Error impl.
                let mut found = false;

                // 1. Check derive attributes above the definition.
                for j in (0..i).rev() {
                    let prev = lines[j].trim();
                    if prev.starts_with("#[derive(")
                        && (prev.contains("thiserror::Error") || prev.contains("Error"))
                    {
                        found = true;
                        break;
                    }
                    if !prev.is_empty()
                        && !prev.starts_with("#[")
                        && !prev.starts_with("///")
                        && !prev.starts_with("//!")
                    {
                        break;
                    }
                }

                // 2. Check for manual `impl ... Error for TypeName` in the file.
                if !found {
                    let pattern = format!("for {name}");
                    for other_line in &lines {
                        if other_line.contains("std::error::Error") && other_line.contains(&pattern)
                        {
                            found = true;
                            break;
                        }
                        if other_line.contains("impl Error") && other_line.contains(&pattern) {
                            found = true;
                            break;
                        }
                    }
                }

                if !found {
                    bad.push(format!("{}:{}: {}", path.display(), i + 1, trimmed));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "error types not implementing std::error::Error:\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn verify_deny_toml_exists_and_complete() {
    let path = workspace_root().join("deny.toml");
    assert!(path.exists(), "deny.toml must exist in workspace root");
    let content = fs::read_to_string(&path).expect("read deny.toml");
    assert!(
        content.contains("[licenses]"),
        "deny.toml must have [licenses] section"
    );
    assert!(
        content.contains("[advisories]"),
        "deny.toml must have [advisories] section"
    );
    assert!(
        content.contains("[bans]"),
        "deny.toml must have [bans] section"
    );
}

#[test]
fn verify_no_wildcard_deps() {
    let mut bad = Vec::new();
    for member in workspace_members() {
        let manifest = crate_manifest(&member);
        if let Some(dep_table) = manifest.get("dependencies").and_then(|d| d.as_table()) {
            for (name, val) in dep_table {
                match val {
                    toml::Value::String(v) if v == "*" => {
                        bad.push(format!("{member}: {name} = \"*\""));
                    }
                    toml::Value::Table(t) => {
                        if let Some(toml::Value::String(v)) = t.get("version")
                            && v == "*"
                        {
                            bad.push(format!("{member}: {name}.version = \"*\""));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(bad.is_empty(), "wildcard dependencies found: {bad:?}");
}

#[test]
fn verify_source_files_are_valid_utf8() {
    let mut bad = Vec::new();
    for member in workspace_members() {
        for path in source_files(&member) {
            if fs::read_to_string(&path).is_err() {
                bad.push(path.display().to_string());
            }
        }
    }
    assert!(bad.is_empty(), "source files with invalid UTF-8: {bad:?}");
}

#[test]
fn verify_no_std_process_exit_in_library_crates() {
    let mut hits = Vec::new();
    for member in workspace_members() {
        // Skip binary crates — they may legitimately call process::exit.
        let main_rs = workspace_root().join(&member).join("src").join("main.rs");
        let is_bin_crate = main_rs.exists();

        for path in source_files(&member) {
            // Skip main.rs itself in binary crates.
            if is_bin_crate && path.ends_with("main.rs") {
                continue;
            }
            let content = fs::read_to_string(&path).expect("read source file");
            for (i, line) in content.lines().enumerate() {
                if line.contains("process::exit") || line.contains("std::process::exit") {
                    hits.push(format!("{}:{}", path.display(), i + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "process::exit found in library code (use Result instead):\n  {}",
        hits.join("\n  ")
    );
}
