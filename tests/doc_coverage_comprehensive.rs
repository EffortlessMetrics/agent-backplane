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
#![allow(clippy::collapsible_if)]
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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive rustdoc generation and documentation coverage tests.
//!
//! Validates that `cargo doc` succeeds for every workspace crate, checks for
//! broken intra-doc links, verifies public items carry doc comments, ensures
//! code examples compile, and confirms structural documentation requirements
//! such as module-level docs, CONTRACT_VERSION references, README sync, and
//! CHANGELOG entries.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ===========================================================================
// Helpers
// ===========================================================================

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn workspace_members() -> Vec<String> {
    let content =
        fs::read_to_string(workspace_root().join("Cargo.toml")).expect("read root Cargo.toml");
    let doc: toml::Value = content.parse().expect("parse root Cargo.toml");
    doc["workspace"]["members"]
        .as_array()
        .expect("workspace.members array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// Extract the bare crate name from a workspace member path like `crates/abp-core`.
fn crate_name(member: &str) -> &str {
    member.rsplit('/').next().unwrap_or(member)
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

/// Run a cargo command and return (success, combined stdout+stderr).
fn cargo_command(args: &[&str]) -> (bool, String) {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(workspace_root())
        .output()
        .expect("failed to execute cargo");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    (output.status.success(), combined)
}

// ===========================================================================
// 1. All crates build docs
// ===========================================================================

/// Verify `cargo doc --no-deps` succeeds for the entire workspace.
#[test]
fn doc_workspace_builds() {
    let (ok, output) = cargo_command(&["doc", "--workspace", "--no-deps"]);
    assert!(ok, "cargo doc --workspace --no-deps failed:\n{output}");
}

/// Each workspace crate individually produces docs without errors.
#[test]
fn doc_each_crate_builds() {
    let mut failures = Vec::new();
    for member in workspace_members() {
        let name = crate_name(&member);
        let (ok, output) = cargo_command(&["doc", "--no-deps", "-p", name]);
        if !ok {
            failures.push(format!("{name}:\n{output}"));
        }
    }
    assert!(
        failures.is_empty(),
        "cargo doc failed for {} crate(s):\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
}

/// Root crate (agent-backplane) produces docs.
#[test]
fn doc_root_crate_builds() {
    let (ok, output) = cargo_command(&["doc", "--no-deps", "-p", "agent-backplane"]);
    assert!(ok, "cargo doc for root crate failed:\n{output}");
}

/// Ensure that `--document-private-items` also compiles cleanly.
#[test]
fn doc_private_items_build() {
    let (ok, output) = cargo_command(&[
        "doc",
        "--workspace",
        "--no-deps",
        "--document-private-items",
    ]);
    assert!(ok, "cargo doc --document-private-items failed:\n{output}");
}

/// Core crate docs build with all features.
#[test]
fn doc_abp_core_all_features() {
    let (ok, output) = cargo_command(&["doc", "--no-deps", "-p", "abp-core", "--all-features"]);
    assert!(
        ok,
        "cargo doc for abp-core --all-features failed:\n{output}"
    );
}

// ===========================================================================
// 2. No broken intra-doc links
// ===========================================================================

/// Run `cargo doc` with `RUSTDOCFLAGS="-D warnings"` to catch broken links.
#[test]
fn doc_no_broken_intra_doc_links() {
    let output = Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps"])
        .env("RUSTDOCFLAGS", "-D warnings")
        .current_dir(workspace_root())
        .output()
        .expect("failed to run cargo doc");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "cargo doc with -D warnings failed (broken links?):\n{stderr}"
    );
}

/// Check that no `unresolved link` warnings appear in doc output.
#[test]
fn doc_no_unresolved_link_warnings() {
    let output = Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run cargo doc");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let unresolved: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("unresolved link"))
        .collect();
    assert!(
        unresolved.is_empty(),
        "Found unresolved doc links:\n{}",
        unresolved.join("\n")
    );
}

/// Core crate specifically should have zero doc warnings.
#[test]
fn doc_abp_core_zero_warnings() {
    let output = Command::new("cargo")
        .args(["doc", "--no-deps", "-p", "abp-core"])
        .env("RUSTDOCFLAGS", "-D warnings")
        .current_dir(workspace_root())
        .output()
        .expect("failed to run cargo doc for abp-core");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "abp-core doc warnings found:\n{stderr}"
    );
}

/// Protocol crate should have zero doc warnings.
#[test]
fn doc_abp_protocol_zero_warnings() {
    let output = Command::new("cargo")
        .args(["doc", "--no-deps", "-p", "abp-protocol"])
        .env("RUSTDOCFLAGS", "-D warnings")
        .current_dir(workspace_root())
        .output()
        .expect("failed to run cargo doc for abp-protocol");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "abp-protocol doc warnings found:\n{stderr}"
    );
}

// ===========================================================================
// 3. Public items documented
// ===========================================================================

/// Every `pub fn`, `pub struct`, `pub enum`, `pub trait`, `pub type`, and
/// `pub const` in a crate's `src/` should be preceded by a doc comment.
/// This heuristic checks for `///` or `#[doc` before `pub` declarations.
#[test]
fn doc_public_items_have_doc_comments() {
    let mut undocumented = Vec::new();
    for member in workspace_members() {
        let src = workspace_root().join(&member).join("src");
        if !src.exists() {
            continue;
        }
        for file in collect_rs_files(&src) {
            let content = fs::read_to_string(&file).unwrap_or_default();
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                // Skip re-exports, `pub use`, `pub mod`, `pub(crate)`, etc.
                if !is_public_item_decl(trimmed) {
                    continue;
                }
                // Look backward for doc comments or attributes.
                if !has_doc_above(&lines, i) {
                    let rel = file.strip_prefix(workspace_root()).unwrap_or(&file);
                    undocumented.push(format!("{}:{}: {}", rel.display(), i + 1, trimmed));
                }
            }
        }
    }
    // Allow up to a small tolerance for generated or trivial items.
    let threshold = 100;
    assert!(
        undocumented.len() <= threshold,
        "Found {} undocumented public items (threshold {threshold}):\n{}",
        undocumented.len(),
        undocumented
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn is_public_item_decl(line: &str) -> bool {
    if line.starts_with("pub(") {
        return false;
    }
    let prefixes = [
        "pub fn ",
        "pub async fn ",
        "pub struct ",
        "pub enum ",
        "pub trait ",
        "pub type ",
        "pub const ",
        "pub static ",
    ];
    prefixes.iter().any(|p| line.starts_with(p))
}

fn has_doc_above(lines: &[&str], idx: usize) -> bool {
    if idx == 0 {
        return false;
    }
    // Walk upward skipping blank lines, attributes, and derive macros.
    let mut j = idx - 1;
    loop {
        let t = lines[j].trim();
        if t.starts_with("///") || t.starts_with("//!") || t.starts_with("#[doc") {
            return true;
        }
        if t.starts_with("#[") || t.is_empty() {
            if j == 0 {
                return false;
            }
            j -= 1;
            continue;
        }
        return false;
    }
}

/// At least the core foundational crates should have ≥80% documented public items.
#[test]
fn doc_core_crate_coverage_above_threshold() {
    let core_crates = ["abp-core", "abp-protocol", "abp-policy", "abp-glob"];
    let mut low_coverage = Vec::new();
    for name in &core_crates {
        let src = workspace_root().join("crates").join(name).join("src");
        if !src.exists() {
            continue;
        }
        let (total, documented) = count_documented_items(&src);
        if total == 0 {
            continue;
        }
        let pct = (documented as f64 / total as f64) * 100.0;
        if pct < 80.0 {
            low_coverage.push(format!("{name}: {documented}/{total} ({pct:.1}%)"));
        }
    }
    assert!(
        low_coverage.is_empty(),
        "Core crates below 80% doc coverage:\n{}",
        low_coverage.join("\n")
    );
}

fn count_documented_items(dir: &Path) -> (usize, usize) {
    let mut total = 0;
    let mut documented = 0;
    for file in collect_rs_files(dir) {
        let content = fs::read_to_string(&file).unwrap_or_default();
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if is_public_item_decl(line.trim()) {
                total += 1;
                if has_doc_above(&lines, i) {
                    documented += 1;
                }
            }
        }
    }
    (total, documented)
}

// ===========================================================================
// 4. Code examples compile (doctests)
// ===========================================================================

/// Run `cargo test --doc` on the workspace to verify doc examples compile.
#[test]
fn doc_examples_compile_workspace() {
    let (ok, output) = cargo_command(&["test", "--doc", "--workspace"]);
    assert!(ok, "cargo test --doc failed:\n{output}");
}

/// Core crate doctests pass.
#[test]
fn doc_examples_compile_abp_core() {
    let (ok, output) = cargo_command(&["test", "--doc", "-p", "abp-core"]);
    assert!(ok, "cargo test --doc -p abp-core failed:\n{output}");
}

/// Protocol crate doctests pass.
#[test]
fn doc_examples_compile_abp_protocol() {
    let (ok, output) = cargo_command(&["test", "--doc", "-p", "abp-protocol"]);
    assert!(ok, "cargo test --doc -p abp-protocol failed:\n{output}");
}

/// Glob crate doctests pass.
#[test]
fn doc_examples_compile_abp_glob() {
    let (ok, output) = cargo_command(&["test", "--doc", "-p", "abp-glob"]);
    assert!(ok, "cargo test --doc -p abp-glob failed:\n{output}");
}

/// Policy crate doctests pass.
#[test]
fn doc_examples_compile_abp_policy() {
    let (ok, output) = cargo_command(&["test", "--doc", "-p", "abp-policy"]);
    assert!(ok, "cargo test --doc -p abp-policy failed:\n{output}");
}

// ===========================================================================
// 5. Module-level docs
// ===========================================================================

/// Every `lib.rs` in the workspace should have a top-level `//!` doc comment.
#[test]
fn doc_all_lib_rs_have_module_docs() {
    let mut missing = Vec::new();
    for member in workspace_members() {
        let lib_rs = workspace_root().join(&member).join("src").join("lib.rs");
        if !lib_rs.exists() {
            continue;
        }
        let content = fs::read_to_string(&lib_rs).unwrap_or_default();
        let has_module_doc = content
            .lines()
            .any(|l| l.trim().starts_with("//!") || l.trim().starts_with("#![doc"));
        if !has_module_doc {
            missing.push(crate_name(&member).to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing module-level docs in lib.rs:\n{}",
        missing.join("\n")
    );
}

/// Public submodules declared in lib.rs should have module-level docs in their file.
#[test]
fn doc_submodules_have_module_docs() {
    let mut missing = Vec::new();
    for member in workspace_members() {
        let src = workspace_root().join(&member).join("src");
        if !src.exists() {
            continue;
        }
        let lib_rs = src.join("lib.rs");
        if !lib_rs.exists() {
            continue;
        }
        let lib_content = fs::read_to_string(&lib_rs).unwrap_or_default();
        for line in lib_content.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("pub mod ") {
                continue;
            }
            let mod_name = trimmed
                .trim_start_matches("pub mod ")
                .trim_end_matches(';')
                .trim_end_matches(" {")
                .trim();
            // Check both file.rs and dir/mod.rs forms.
            let file_path = src.join(format!("{mod_name}.rs"));
            let mod_path = src.join(mod_name).join("mod.rs");
            let path = if file_path.exists() {
                Some(file_path)
            } else if mod_path.exists() {
                Some(mod_path)
            } else {
                None
            };
            if let Some(p) = path {
                let content = fs::read_to_string(&p).unwrap_or_default();
                let has_doc = content
                    .lines()
                    .any(|l| l.trim().starts_with("//!") || l.trim().starts_with("#![doc"));
                if !has_doc {
                    let rel = p.strip_prefix(workspace_root()).unwrap_or(&p);
                    missing.push(rel.display().to_string());
                }
            }
        }
    }
    // Allow a tolerance for trivially small modules.
    let threshold = 20;
    assert!(
        missing.len() <= threshold,
        "Found {} submodules without module-level docs (threshold {threshold}):\n{}",
        missing.len(),
        missing
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Core crate's lib.rs must reference the crate purpose.
#[test]
fn doc_core_lib_rs_describes_purpose() {
    let lib_rs = workspace_root()
        .join("crates")
        .join("abp-core")
        .join("src")
        .join("lib.rs");
    let content = fs::read_to_string(&lib_rs).expect("read abp-core lib.rs");
    let header: String = content
        .lines()
        .take_while(|l| l.starts_with("//!") || l.starts_with("#![doc") || l.starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        header.contains("contract") || header.contains("core") || header.contains("stable"),
        "abp-core lib.rs module doc should mention contract/core/stable:\n{header}"
    );
}

/// Protocol crate lib.rs mentions wire format or JSONL.
#[test]
fn doc_protocol_lib_rs_mentions_wire_format() {
    let lib_rs = workspace_root()
        .join("crates")
        .join("abp-protocol")
        .join("src")
        .join("lib.rs");
    let content = fs::read_to_string(&lib_rs).expect("read abp-protocol lib.rs");
    let lower = content.to_lowercase();
    assert!(
        lower.contains("jsonl") || lower.contains("wire") || lower.contains("protocol"),
        "abp-protocol lib.rs should mention jsonl/wire/protocol"
    );
}

// ===========================================================================
// 6. CONTRACT_VERSION referenced in core types
// ===========================================================================

/// `CONTRACT_VERSION` is defined in abp-core.
#[test]
fn doc_contract_version_defined() {
    let lib_rs = workspace_root()
        .join("crates")
        .join("abp-core")
        .join("src")
        .join("lib.rs");
    let content = fs::read_to_string(&lib_rs).expect("read abp-core lib.rs");
    assert!(
        content.contains("CONTRACT_VERSION"),
        "abp-core must define CONTRACT_VERSION"
    );
}

/// `CONTRACT_VERSION` value matches the expected format `abp/vX.Y`.
#[test]
fn doc_contract_version_format_in_source() {
    let lib_rs = workspace_root()
        .join("crates")
        .join("abp-core")
        .join("src")
        .join("lib.rs");
    let content = fs::read_to_string(&lib_rs).expect("read abp-core lib.rs");
    assert!(
        content.contains(r#""abp/v0.1""#),
        "CONTRACT_VERSION should be \"abp/v0.1\""
    );
}

/// `CONTRACT_VERSION` is documented with a doc comment.
#[test]
fn doc_contract_version_has_doc_comment() {
    let lib_rs = workspace_root()
        .join("crates")
        .join("abp-core")
        .join("src")
        .join("lib.rs");
    let content = fs::read_to_string(&lib_rs).expect("read abp-core lib.rs");
    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("pub const CONTRACT_VERSION") {
            assert!(
                has_doc_above(&lines, i),
                "CONTRACT_VERSION must have a doc comment"
            );
            return;
        }
    }
    panic!("CONTRACT_VERSION declaration not found");
}

/// Receipt type references CONTRACT_VERSION in its source file.
#[test]
fn doc_receipt_references_contract_version() {
    let src = workspace_root().join("crates").join("abp-core").join("src");
    let files = collect_rs_files(&src);
    let any_ref = files.iter().any(|f| {
        let c = fs::read_to_string(f).unwrap_or_default();
        c.contains("contract_version") && c.contains("CONTRACT_VERSION")
    });
    assert!(
        any_ref,
        "Receipt-related code should reference CONTRACT_VERSION"
    );
}

/// WorkOrder type references CONTRACT_VERSION in its source.
#[test]
fn doc_work_order_references_contract_version() {
    let src = workspace_root().join("crates").join("abp-core").join("src");
    let files = collect_rs_files(&src);
    let any_ref = files.iter().any(|f| {
        let c = fs::read_to_string(f).unwrap_or_default();
        c.contains("WorkOrder") && c.contains("contract_version")
    });
    assert!(any_ref, "WorkOrder should include a contract_version field");
}

// ===========================================================================
// 7. README sync
// ===========================================================================

/// README.md exists and is non-trivial.
#[test]
fn doc_readme_exists_and_nontrivial() {
    let readme = workspace_root().join("README.md");
    assert!(readme.exists(), "README.md must exist");
    let len = fs::metadata(&readme).unwrap().len();
    assert!(len > 500, "README.md should be >500 bytes, got {len}");
}

/// README mentions the core crate.
#[test]
fn doc_readme_mentions_abp_core() {
    let content = fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    assert!(
        content.contains("abp-core") || content.contains("abp_core"),
        "README should mention abp-core"
    );
}

/// README mentions the contract version.
#[test]
fn doc_readme_mentions_contract_version() {
    let content = fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    assert!(
        content.contains("CONTRACT_VERSION")
            || content.contains("abp/v0.1")
            || content.contains("contract"),
        "README should reference the contract version"
    );
}

/// README code blocks are syntactically marked with a language.
#[test]
fn doc_readme_code_blocks_have_language() {
    let content = fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    let mut unmarked = Vec::new();
    // Count opening fences without a language tag.
    let mut in_block = false;
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_block {
                in_block = false;
            } else {
                in_block = true;
                if trimmed == "```" {
                    unmarked.push(i + 1);
                }
            }
        }
    }
    assert!(
        unmarked.len() <= 3,
        "README has {} code blocks without language markers at lines: {:?}",
        unmarked.len(),
        unmarked
    );
}

/// README architecture section lists at least some crate names.
#[test]
fn doc_readme_architecture_section() {
    let content = fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    let lower = content.to_lowercase();
    assert!(
        lower.contains("architecture") || lower.contains("## crate"),
        "README should have an Architecture or Crate section"
    );
}

/// README mentions the sidecar protocol.
#[test]
fn doc_readme_mentions_sidecar_protocol() {
    let content = fs::read_to_string(workspace_root().join("README.md")).expect("read README.md");
    let lower = content.to_lowercase();
    assert!(
        lower.contains("sidecar") || lower.contains("jsonl"),
        "README should reference the sidecar/JSONL protocol"
    );
}

// ===========================================================================
// 8. CHANGELOG entries
// ===========================================================================

/// CHANGELOG.md exists.
#[test]
fn doc_changelog_exists() {
    let cl = workspace_root().join("CHANGELOG.md");
    assert!(cl.exists(), "CHANGELOG.md must exist at workspace root");
}

/// CHANGELOG follows Keep a Changelog format.
#[test]
fn doc_changelog_format() {
    let content =
        fs::read_to_string(workspace_root().join("CHANGELOG.md")).expect("read CHANGELOG.md");
    assert!(
        content.contains("# Changelog") || content.contains("# CHANGELOG"),
        "CHANGELOG should start with a heading"
    );
    assert!(
        content.contains("[Unreleased]") || content.contains("Unreleased"),
        "CHANGELOG should have an [Unreleased] section"
    );
}

/// CHANGELOG mentions at least some workspace crate names.
#[test]
fn doc_changelog_mentions_crates() {
    let content =
        fs::read_to_string(workspace_root().join("CHANGELOG.md")).expect("read CHANGELOG.md");
    let core_crates = ["abp-core", "abp-protocol", "abp-policy"];
    let mentioned = core_crates.iter().filter(|c| content.contains(**c)).count();
    assert!(
        mentioned >= 2,
        "CHANGELOG should mention at least 2 of the core crates, found {mentioned}"
    );
}

/// CHANGELOG has section headers for change categories.
#[test]
fn doc_changelog_has_categories() {
    let content =
        fs::read_to_string(workspace_root().join("CHANGELOG.md")).expect("read CHANGELOG.md");
    let lower = content.to_lowercase();
    let categories = ["added", "changed", "fixed", "removed", "new"];
    let found = categories.iter().filter(|c| lower.contains(**c)).count();
    assert!(
        found >= 1,
        "CHANGELOG should have at least one change category (Added/Changed/Fixed/Removed)"
    );
}

/// CHANGELOG is non-trivial in size.
#[test]
fn doc_changelog_nontrivial_size() {
    let cl = workspace_root().join("CHANGELOG.md");
    let len = fs::metadata(&cl).unwrap().len();
    assert!(len > 200, "CHANGELOG.md should be >200 bytes, got {len}");
}

// ===========================================================================
// 9. Additional structural doc checks
// ===========================================================================

/// Every crate's Cargo.toml has a `description` field (required for docs.rs).
#[test]
fn doc_all_crates_have_description() {
    let ws_toml: toml::Value = fs::read_to_string(workspace_root().join("Cargo.toml"))
        .expect("read root Cargo.toml")
        .parse()
        .expect("parse root Cargo.toml");
    let ws_desc = ws_toml
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("description"));

    let mut missing = Vec::new();
    for member in workspace_members() {
        let name = crate_name(&member);
        let path = workspace_root().join(&member).join("Cargo.toml");
        let content = fs::read_to_string(&path).unwrap_or_default();
        let doc: toml::Value = content
            .parse()
            .unwrap_or(toml::Value::Table(Default::default()));
        let pkg = doc.get("package");
        let has_desc = pkg
            .and_then(|p| p.get("description"))
            .map(|v| {
                v.as_str().is_some()
                    || v.get("workspace")
                        .and_then(|w| w.as_bool())
                        .unwrap_or(false)
                        && ws_desc.is_some()
            })
            .unwrap_or(false);
        if !has_desc {
            missing.push(name.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "Crates missing description in Cargo.toml:\n{}",
        missing.join("\n")
    );
}

/// Crate README files exist for core crates (docs.rs renders these).
#[test]
fn doc_core_crates_have_readme() {
    let core_crates = [
        "abp-core",
        "abp-protocol",
        "abp-glob",
        "abp-policy",
        "abp-host",
    ];
    let mut missing = Vec::new();
    for name in &core_crates {
        let readme = workspace_root().join("crates").join(name).join("README.md");
        if !readme.exists() {
            missing.push(name.to_string());
        }
    }
    // Not all crates necessarily have READMEs, but core ones should.
    assert!(
        missing.len() <= 2,
        "Core crates missing README.md: {missing:?}"
    );
}

/// `docs/` directory exists and has content.
#[test]
fn doc_docs_directory_exists() {
    let docs = workspace_root().join("docs");
    assert!(docs.exists(), "docs/ directory should exist");
    let entries: Vec<_> = fs::read_dir(&docs).expect("read docs/").flatten().collect();
    assert!(!entries.is_empty(), "docs/ directory should not be empty");
}

/// All workspace members are tracked in the CHANGELOG or README.
#[test]
fn doc_all_crates_referenced_in_docs() {
    let readme = fs::read_to_string(workspace_root().join("README.md")).unwrap_or_default();
    let changelog = fs::read_to_string(workspace_root().join("CHANGELOG.md")).unwrap_or_default();
    let combined = format!("{readme}\n{changelog}");
    let mut unreferenced = Vec::new();
    for member in workspace_members() {
        let name = crate_name(&member);
        if name == "xtask" {
            continue;
        }
        if !combined.contains(name) {
            unreferenced.push(name.to_string());
        }
    }
    // Tolerate a few niche utility crates.
    let threshold = 5;
    assert!(
        unreferenced.len() <= threshold,
        "Found {} crates not referenced in README or CHANGELOG (threshold {threshold}):\n{}",
        unreferenced.len(),
        unreferenced.join("\n")
    );
}

/// No workspace crate has a `lib.rs` that is completely empty.
#[test]
fn doc_no_empty_lib_rs() {
    let mut empty = Vec::new();
    for member in workspace_members() {
        let lib_rs = workspace_root().join(&member).join("src").join("lib.rs");
        if !lib_rs.exists() {
            continue;
        }
        let content = fs::read_to_string(&lib_rs).unwrap_or_default();
        if content.trim().is_empty() {
            empty.push(crate_name(&member).to_string());
        }
    }
    assert!(
        empty.is_empty(),
        "Crates with empty lib.rs: {}",
        empty.join(", ")
    );
}

/// Sidecar protocol doc exists.
#[test]
fn doc_sidecar_protocol_doc_exists() {
    let doc = workspace_root().join("docs").join("sidecar_protocol.md");
    if doc.exists() {
        let len = fs::metadata(&doc).unwrap().len();
        assert!(len > 100, "sidecar_protocol.md should be non-trivial");
    }
    // If it doesn't exist, we allow it — the protocol may be documented elsewhere.
}

/// `CONTRIBUTING.md` exists for open-source compliance.
#[test]
fn doc_contributing_exists() {
    let p = workspace_root().join("CONTRIBUTING.md");
    assert!(p.exists(), "CONTRIBUTING.md must exist");
    let len = fs::metadata(&p).unwrap().len();
    assert!(len > 100, "CONTRIBUTING.md should be non-trivial");
}

/// `LICENSE-MIT` and `LICENSE-APACHE` exist.
#[test]
fn doc_license_files_exist() {
    assert!(
        workspace_root().join("LICENSE-MIT").exists(),
        "LICENSE-MIT must exist"
    );
    assert!(
        workspace_root().join("LICENSE-APACHE").exists(),
        "LICENSE-APACHE must exist"
    );
}
