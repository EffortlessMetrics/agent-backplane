// SPDX-License-Identifier: MIT OR Apache-2.0
use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use globset::{Glob, GlobSetBuilder};
use schemars::schema_for;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command as Cmd;

#[derive(Parser, Debug)]
#[command(name = "xtask", version, about = "Repo maintenance tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate JSON Schemas for contract types.
    Schema {
        /// Output directory.
        #[arg(long, default_value = "contracts/schemas")]
        out_dir: PathBuf,
    },
    /// Run full CI checks locally (fmt, clippy, test, doc-test).
    Check,
    /// Print instructions for running code coverage with tarpaulin.
    Coverage,
    /// Run formatting and clippy checks only.
    Lint,
    /// Auto-fix formatting and clippy issues (mutating by default).
    LintFix {
        /// Run in check mode (non-mutating, CI parity).
        #[arg(long)]
        check: bool,
        /// Skip clippy --fix (only format).
        #[arg(long)]
        no_clippy: bool,
    },
    /// Pre-push gate: fmt + cargo check + clippy + test compile (no test execution).
    Gate {
        /// Strict check mode (all steps non-mutating, CI parity).
        #[arg(long)]
        check: bool,
    },
    /// Verify crates.io release readiness.
    ReleaseCheck,
    /// Build workspace documentation.
    Docs {
        /// Open documentation in browser after building.
        #[arg(long)]
        open: bool,
    },
    /// List all workspace crates with their paths.
    ListCrates,
    /// Run workspace quality checks (required fields, unused deps, version consistency).
    Audit,
    /// Show workspace statistics (crates, tests, LOC, dependency depth).
    Stats,
    /// Configure local repo for development (install git hooks).
    Setup,
    /// Verify workspace lint policy, inheritance, debt, and planned flips.
    CheckLintPolicy,
    /// Verify panic-family allowlist schema and expiry.
    CheckNoPanicFamily,
    /// Verify non-Rust file policy allowlist schema and coverage.
    CheckFilePolicy,
    /// Print a concise policy exception report.
    PolicyReport,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    warn_if_hooks_missing();
    match cli.command {
        Command::Schema { out_dir } => schema(out_dir),
        Command::Check => check(),
        Command::Coverage => coverage(),
        Command::Lint => lint(),
        Command::LintFix { check, no_clippy } => lint_fix(check, no_clippy),
        Command::Gate { check } => gate(check),
        Command::ReleaseCheck => release_check(),
        Command::Docs { open } => docs(open),
        Command::ListCrates => list_crates(),
        Command::Audit => audit(),
        Command::Stats => stats(),
        Command::Setup => setup(),
        Command::CheckLintPolicy => check_lint_policy(),
        Command::CheckNoPanicFamily => check_no_panic_family(),
        Command::CheckFilePolicy => check_file_policy(),
        Command::PolicyReport => policy_report(),
    }
}

// ── schema ───────────────────────────────────────────────────────────

fn schema(out_dir: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&out_dir).context("create schema output dir")?;

    let wo = schema_for!(abp_core::WorkOrder);
    let receipt = schema_for!(abp_core::Receipt);
    let config = schema_for!(abp_cli::config::BackplaneConfig);

    write_schema(&out_dir.join("work_order.schema.json"), &wo)?;
    write_schema(&out_dir.join("receipt.schema.json"), &receipt)?;
    write_schema(&out_dir.join("backplane_config.schema.json"), &config)?;

    eprintln!("wrote schemas to {}", out_dir.display());
    Ok(())
}

fn write_schema(path: &PathBuf, schema: &schemars::Schema) -> Result<()> {
    let s = serde_json::to_string_pretty(schema)?;
    std::fs::write(path, s).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

// ── check ────────────────────────────────────────────────────────────

fn run_cargo(args: &[&str]) -> Result<()> {
    eprintln!("→ cargo {}", args.join(" "));
    let status = Cmd::new("cargo")
        .args(args)
        .status()
        .with_context(|| format!("spawn cargo {}", args.join(" ")))?;
    anyhow::ensure!(
        status.success(),
        "cargo {} failed ({})",
        args.join(" "),
        status
    );
    Ok(())
}

/// Run `cargo fmt` with optional check mode.
/// Falls back to per-package formatting on Windows if `--all` fails (OS error 206: path too long).
fn run_fmt(check: bool) -> Result<()> {
    let args: Vec<&str> = if check {
        vec!["fmt", "--all", "--", "--check"]
    } else {
        vec!["fmt", "--all"]
    };

    let result = run_cargo(&args);

    if result.is_ok() || !cfg!(windows) {
        return result;
    }

    // Windows fallback: per-package formatting to avoid path-length errors (OS error 206)
    eprintln!("→ fmt --all failed on Windows; falling back to per-package formatting");
    let output = Cmd::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .context("run cargo metadata")?;
    anyhow::ensure!(output.status.success(), "cargo metadata failed");

    let meta: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parse cargo metadata")?;
    let packages = meta["packages"]
        .as_array()
        .context("cargo metadata missing packages")?;

    for pkg in packages {
        let name = pkg["name"].as_str().context("package missing name")?;
        let mut pkg_args = vec!["fmt", "-p", name];
        if check {
            pkg_args.extend(["--", "--check"]);
        }
        if run_cargo(&pkg_args).is_err() {
            // Per-package fmt can also fail for the workspace root package;
            // fall back to running rustfmt directly on the package's source files.
            let manifest_path = pkg["manifest_path"]
                .as_str()
                .context("package missing manifest_path")?;
            let pkg_dir = std::path::Path::new(manifest_path)
                .parent()
                .context("manifest_path has no parent")?;
            let src_dir = pkg_dir.join("src");
            if src_dir.exists() {
                let rs_files: Vec<PathBuf> = walk_rs_files(&src_dir).collect();
                if !rs_files.is_empty() {
                    eprintln!("→ falling back to direct rustfmt for {name}");
                    let mut cmd = Cmd::new("rustfmt");
                    if check {
                        cmd.arg("--check");
                    }
                    for f in &rs_files {
                        cmd.arg(f);
                    }
                    let status = cmd.status().context("run rustfmt")?;
                    anyhow::ensure!(status.success(), "rustfmt failed for {name}");
                }
            }
        }
    }

    Ok(())
}

fn check() -> Result<()> {
    // Use run_fmt for Windows fallback support
    let fmt_ok = run_fmt(true).is_ok();

    let steps: &[(&str, &[&str])] = &[
        (
            "clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("test", &["test", "--workspace"]),
        ("doc-test", &["test", "--doc", "--workspace"]),
    ];

    let mut results: Vec<(&str, bool)> = vec![("fmt", fmt_ok)];
    for (name, args) in steps {
        let ok = run_cargo(args).is_ok();
        results.push((name, ok));
    }

    eprintln!();
    eprintln!("── summary ─────────────────────────");
    let mut all_passed = true;
    for (name, ok) in &results {
        let icon = if *ok { "✓" } else { "✗" };
        eprintln!("  {icon} {name}");
        if !*ok {
            all_passed = false;
        }
    }
    eprintln!();

    if all_passed {
        eprintln!("all checks passed ✓");
        Ok(())
    } else {
        anyhow::bail!("some checks failed");
    }
}

// ── coverage ─────────────────────────────────────────────────────────

fn coverage() -> Result<()> {
    // Try to invoke tarpaulin; if not installed, print instructions.
    let found = Cmd::new("cargo")
        .args(["tarpaulin", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if found {
        run_cargo(&["tarpaulin", "--workspace", "--out", "html"])?;
    } else {
        eprintln!("cargo-tarpaulin is not installed.");
        eprintln!();
        eprintln!("Install it with:");
        eprintln!("  cargo install cargo-tarpaulin");
        eprintln!();
        eprintln!("Then run:");
        eprintln!("  cargo tarpaulin --workspace --out html");
    }
    Ok(())
}

// ── lint ──────────────────────────────────────────────────────────────

fn lint() -> Result<()> {
    run_fmt(true)?;
    run_cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
    ])?;
    eprintln!("lint passed ✓");
    Ok(())
}

// ── lint-fix ─────────────────────────────────────────────────────────

fn lint_fix(check: bool, no_clippy: bool) -> Result<()> {
    if check {
        // Check-only mode (non-mutating)
        run_fmt(true)?;
        if !no_clippy {
            run_cargo(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ])?;
        }
    } else {
        // Mutating mode: fix first, then verify
        run_fmt(false)?;
        if !no_clippy {
            // Best-effort clippy fix
            let _ = run_cargo(&[
                "clippy",
                "--fix",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--allow-dirty",
                "--allow-staged",
                "--",
                "-D",
                "warnings",
            ]);
            // Verify clean
            run_cargo(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ])?;
        }
    }
    eprintln!("lint-fix passed ✓");
    Ok(())
}

// ── gate ─────────────────────────────────────────────────────────────

fn gate(check: bool) -> Result<()> {
    if check {
        // Strict CI-parity mode: everything non-mutating
        run_fmt(true)?;
    } else {
        // Local dev mode: fix fmt first, then verify the rest
        run_fmt(false)?;
    }
    // Warm dependency graph
    run_cargo(&["check", "--workspace", "--all-targets", "--all-features"])?;
    // Clippy
    run_cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
    ])?;
    // Compile tests without running them
    run_cargo(&["test", "--workspace", "--no-run"])?;
    eprintln!("gate passed ✓");
    Ok(())
}

// ── release-check ────────────────────────────────────────────────────

fn release_check() -> Result<()> {
    let root = workspace_root()?;
    let ws_manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).context("read workspace Cargo.toml")?;
    let ws_doc: toml::Value = ws_manifest.parse().context("parse workspace Cargo.toml")?;

    let ws_version = ws_doc
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .context("workspace.package.version not found")?;

    let members = ws_doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .context("workspace.members not found")?;

    let required_fields = [
        "version",
        "edition",
        "rust-version",
        "license",
        "authors",
        "repository",
        "description",
        "readme",
        "keywords",
        "categories",
    ];
    let mut ok = true;
    for member in members {
        let Some(path) = member.as_str() else {
            continue;
        };
        let crate_toml_path = root.join(path).join("Cargo.toml");
        if !crate_toml_path.exists() {
            eprintln!("  ✗ {path}: Cargo.toml missing");
            ok = false;
            continue;
        }

        let content = std::fs::read_to_string(&crate_toml_path)
            .with_context(|| format!("read {}", crate_toml_path.display()))?;
        let doc: toml::Value = content
            .parse()
            .with_context(|| format!("parse {}", crate_toml_path.display()))?;

        let pkg = doc.get("package");
        let name = pkg
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(path);

        let is_not_publishable = pkg
            .and_then(|p| p.get("publish"))
            .and_then(|p| p.as_bool())
            .is_some_and(|publish| !publish);
        if is_not_publishable {
            continue;
        }

        // Check required fields (may be inherited via `.workspace = true`)
        for field in required_fields {
            if pkg.and_then(|p| p.get(field)).is_none() {
                eprintln!("  ✗ {name}: missing package.{field}");
                ok = false;
            }
        }

        // Check README exists (path comes from manifest or defaults to README.md).
        let readme_path = pkg
            .and_then(|p| p.get("readme"))
            .and_then(|r| r.as_str())
            .map_or_else(
                || root.join(path).join("README.md"),
                |r| root.join(path).join(r),
            );
        if !readme_path.exists() {
            eprintln!("  ✗ {name}: missing README.md");
            ok = false;
        }

        // Check version consistency (explicit versions should match workspace)
        if let Some(ver) = pkg
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
            .filter(|&ver| ver != ws_version)
        {
            eprintln!("  ✗ {name}: version {ver} != workspace {ws_version}");
            ok = false;
        }
    }

    if ok {
        eprintln!(
            "  ✓ all publishable crates have required metadata, READMEs, and consistent versions"
        );
    }

    // Dry-run packaging
    eprintln!();
    eprintln!("running cargo package --workspace --allow-dirty (dry-run)…");
    run_cargo(&["package", "--workspace", "--allow-dirty", "--list"])?;

    if !ok {
        anyhow::bail!("release-check found issues");
    }
    eprintln!("release-check passed ✓");
    Ok(())
}

// ── docs ─────────────────────────────────────────────────────────────

fn docs(open: bool) -> Result<()> {
    let mut args = vec!["doc", "--workspace", "--no-deps"];
    if open {
        args.push("--open");
    }
    run_cargo(&args)?;
    eprintln!("docs built ✓");
    Ok(())
}

// ── list-crates ──────────────────────────────────────────────────────

fn list_crates() -> Result<()> {
    let manifest = workspace_root()?.join("Cargo.toml");
    let content = std::fs::read_to_string(&manifest).context("read workspace Cargo.toml")?;
    let doc: toml::Value = content.parse().context("parse workspace Cargo.toml")?;

    let members = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .context("workspace.members not found")?;

    for member in members {
        if let Some(path) = member.as_str() {
            let crate_toml = workspace_root()?.join(path).join("Cargo.toml");
            let name = if crate_toml.exists() {
                read_crate_name(&crate_toml).unwrap_or_else(|| path.to_string())
            } else {
                path.to_string()
            };
            println!("{name:30} {path}");
        }
    }
    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    let xtask_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    xtask_dir
        .parent()
        .map(PathBuf::from)
        .context("find workspace root")
}

fn read_crate_name(path: &PathBuf) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let doc: toml::Value = content.parse().ok()?;
    doc.get("package")?.get("name")?.as_str().map(String::from)
}

fn read_workspace(root: &std::path::Path) -> Result<(String, Vec<toml::Value>)> {
    let ws_manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).context("read workspace Cargo.toml")?;
    let ws_doc: toml::Value = ws_manifest.parse().context("parse workspace Cargo.toml")?;

    let ws_version = ws_doc
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .context("workspace.package.version not found")?
        .to_string();

    let members = ws_doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .context("workspace.members not found")?
        .clone();

    Ok((ws_version, members))
}

fn walk_rs_files(dir: &std::path::Path) -> impl Iterator<Item = PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
        .map(|e| e.into_path())
}

// ── audit ────────────────────────────────────────────────────────────

fn audit() -> Result<()> {
    let root = workspace_root()?;
    let (ws_version, members) = read_workspace(&root)?;

    let mut issues = 0u32;

    println!("── audit: required fields ──────────────");
    for member in &members {
        let Some(path) = member.as_str() else {
            continue;
        };
        let crate_toml_path = root.join(path).join("Cargo.toml");
        if !crate_toml_path.exists() {
            println!("  ✗ {path}: Cargo.toml missing");
            issues += 1;
            continue;
        }

        let content = std::fs::read_to_string(&crate_toml_path)
            .with_context(|| format!("read {}", crate_toml_path.display()))?;
        let doc: toml::Value = content
            .parse()
            .with_context(|| format!("parse {}", crate_toml_path.display()))?;
        let pkg = doc.get("package");
        let name = pkg
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(path);

        for field in ["name", "version", "edition", "license"] {
            if pkg.and_then(|p| p.get(field)).is_none() {
                println!("  ✗ {name}: missing package.{field}");
                issues += 1;
            }
        }
    }

    println!();
    println!("── audit: version consistency ──────────");
    for member in &members {
        let Some(path) = member.as_str() else {
            continue;
        };
        let crate_toml_path = root.join(path).join("Cargo.toml");
        if !crate_toml_path.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&crate_toml_path)?;
        let doc: toml::Value = content.parse()?;
        let pkg = doc.get("package");
        let name = pkg
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(path);

        if let Some(ver) = pkg
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
            .filter(|&ver| ver != ws_version)
        {
            println!("  ✗ {name}: version {ver} != workspace {ws_version}");
            issues += 1;
        }
    }

    println!();
    println!("── audit: unused dependencies ──────────");
    for member in &members {
        let Some(path) = member.as_str() else {
            continue;
        };
        let crate_toml_path = root.join(path).join("Cargo.toml");
        if !crate_toml_path.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&crate_toml_path)?;
        let doc: toml::Value = content.parse()?;
        let Some(deps) = doc.get("dependencies").and_then(|d| d.as_table()) else {
            continue;
        };

        let src_dir = root.join(path).join("src");
        if !src_dir.exists() {
            continue;
        }

        let mut src_content = String::new();
        for rs_path in walk_rs_files(&src_dir) {
            if let Ok(text) = std::fs::read_to_string(&rs_path) {
                src_content.push_str(&text);
            }
        }

        let name = doc
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(path);

        for dep_name in deps.keys() {
            let ident = dep_name.replace('-', "_");
            if !src_content.contains(&ident) {
                println!("  ? {name}: possibly unused dep '{dep_name}'");
            }
        }
    }

    println!();
    if issues == 0 {
        println!("audit passed ✓ (0 issues)");
    } else {
        println!("audit found {issues} issue(s)");
    }
    Ok(())
}

// ── stats ────────────────────────────────────────────────────────────

fn stats() -> Result<()> {
    let root = workspace_root()?;
    let (_ws_version, members) = read_workspace(&root)?;

    let crate_count = members.len();
    let mut total_lines = 0usize;
    let mut total_test_files = 0usize;
    let mut total_tests = 0usize;

    println!("── workspace statistics ─────────────────");
    println!();
    println!("crates: {crate_count}");
    println!();
    println!(
        "{:<30} {:>8} {:>10} {:>8}",
        "crate", "LOC", "test-files", "#deps"
    );
    println!("{}", "─".repeat(60));

    for member in &members {
        let Some(path) = member.as_str() else {
            continue;
        };
        let crate_dir = root.join(path);
        let crate_toml_path = crate_dir.join("Cargo.toml");

        let name = if crate_toml_path.exists() {
            read_crate_name(&crate_toml_path).unwrap_or_else(|| path.to_string())
        } else {
            path.to_string()
        };

        // Lines of code in src/
        let mut crate_lines = 0usize;
        let src_dir = crate_dir.join("src");
        if src_dir.exists() {
            for rs_path in walk_rs_files(&src_dir) {
                if let Ok(text) = std::fs::read_to_string(&rs_path) {
                    crate_lines += text.lines().count();
                }
            }
        }

        // Test files in tests/
        let mut test_files = 0usize;
        let tests_dir = crate_dir.join("tests");
        if tests_dir.exists() {
            test_files = walk_rs_files(&tests_dir).count();
        }

        // Count #[test] annotations
        let mut test_count = 0usize;
        for dir in [&src_dir, &crate_dir.join("tests")] {
            if dir.exists() {
                for rs_path in walk_rs_files(dir) {
                    if let Ok(text) = std::fs::read_to_string(&rs_path) {
                        test_count += text.matches("#[test]").count();
                        test_count += text.matches("#[tokio::test]").count();
                    }
                }
            }
        }

        // Dependency count
        let dep_count = if crate_toml_path.exists() {
            let content = std::fs::read_to_string(&crate_toml_path).unwrap_or_default();
            let doc: toml::Value = content
                .parse()
                .unwrap_or(toml::Value::Table(Default::default()));
            doc.get("dependencies")
                .and_then(|d| d.as_table())
                .map(|t| t.len())
                .unwrap_or(0)
        } else {
            0
        };

        total_lines += crate_lines;
        total_test_files += test_files;
        total_tests += test_count;

        println!("{name:<30} {crate_lines:>8} {test_files:>10} {dep_count:>8}");
    }

    println!("{}", "─".repeat(60));
    println!(
        "{:<30} {:>8} {:>10}",
        "TOTAL", total_lines, total_test_files
    );
    println!();
    println!("total #[test] functions:     {total_tests}");
    println!(
        "max dependency tree depth:   {}",
        max_dep_depth(&root, &members)?
    );

    Ok(())
}

fn max_dep_depth(root: &std::path::Path, members: &[toml::Value]) -> Result<usize> {
    let mut member_names: HashSet<String> = HashSet::new();
    let mut dep_graph: HashMap<String, Vec<String>> = HashMap::new();

    for member in members {
        let Some(path) = member.as_str() else {
            continue;
        };
        let crate_toml = root.join(path).join("Cargo.toml");
        if !crate_toml.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&crate_toml)?;
        let doc: toml::Value = content.parse()?;
        let name = doc
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(path)
            .to_string();

        member_names.insert(name.clone());

        let deps: Vec<String> = doc
            .get("dependencies")
            .and_then(|d| d.as_table())
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default();

        dep_graph.insert(name, deps);
    }

    let mut cache: HashMap<String, usize> = HashMap::new();
    let mut result = 0;
    for name in &member_names {
        result = result.max(dep_depth(name, &dep_graph, &member_names, &mut cache));
    }
    Ok(result)
}

fn dep_depth(
    name: &str,
    graph: &HashMap<String, Vec<String>>,
    members: &HashSet<String>,
    cache: &mut HashMap<String, usize>,
) -> usize {
    if let Some(&d) = cache.get(name) {
        return d;
    }
    let deps = match graph.get(name) {
        Some(deps) => deps,
        None => return 0,
    };
    let mut max_child = 0;
    for dep in deps {
        if members.contains(dep) {
            max_child = max_child.max(dep_depth(dep, graph, members, cache) + 1);
        }
    }
    cache.insert(name.to_string(), max_child);
    max_child
}

// ── policy checks ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ClippyLintPolicy {
    schema: u64,
    msrv: String,
    policy: ClippyPolicyFlags,
    #[serde(default)]
    lint: Vec<ClippyLintEntry>,
}

#[derive(Debug, Deserialize)]
struct ClippyPolicyFlags {
    panic_free_tests: bool,
    allow_test_carveouts: bool,
    suppression_style: String,
    blanket_categories: bool,
}

#[derive(Debug, Deserialize)]
struct ClippyLintEntry {
    name: String,
    level: String,
    status: String,
    #[serde(default)]
    activate_when_msrv: Option<String>,
    #[serde(default)]
    class: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct ClippyDebtPolicy {
    schema: u64,
    #[serde(default)]
    debt: Vec<ClippyDebtEntry>,
}

#[derive(Debug, Deserialize)]
struct ClippyDebtEntry {
    lint: String,
    path: String,
    owner: String,
    reason: String,
    expires: String,
}

#[derive(Debug, Deserialize)]
struct NoPanicAllowlist {
    schema_version: String,
    #[serde(default)]
    allow: Vec<PanicAllowEntry>,
}

#[derive(Debug, Deserialize)]
struct PanicAllowEntry {
    path: String,
    family: String,
    classification: String,
    owner: String,
    explanation: String,
    #[serde(default)]
    expires: Option<String>,
    selector: toml::Value,
    #[serde(default)]
    last_seen: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
struct NonRustAllowlist {
    schema_version: String,
    #[serde(default)]
    allow: Vec<NonRustAllowEntry>,
}

#[derive(Debug, Deserialize)]
struct NonRustAllowEntry {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    kind: String,
    owner: String,
    reason: String,
    surface: String,
    classification: String,
    #[serde(default)]
    covered_by: Vec<String>,
    #[serde(default)]
    expires: Option<String>,
}

fn check_lint_policy() -> Result<()> {
    let root = workspace_root()?;
    let cargo_doc = read_toml(&root.join("Cargo.toml"))?;
    let lint_policy: ClippyLintPolicy = read_toml_as(&root.join("policy/clippy-lints.toml"))?;
    let debt_policy: ClippyDebtPolicy = read_toml_as(&root.join("policy/clippy-debt.toml"))?;

    let mut issues = Vec::new();
    if lint_policy.schema != 1 {
        issues.push("policy/clippy-lints.toml schema must be 1".to_string());
    }
    if debt_policy.schema != 1 {
        issues.push("policy/clippy-debt.toml schema must be 1".to_string());
    }
    if !lint_policy.policy.panic_free_tests {
        issues.push("policy.panic_free_tests must be true".to_string());
    }
    if lint_policy.policy.allow_test_carveouts {
        issues.push("policy.allow_test_carveouts must be false".to_string());
    }
    if lint_policy.policy.suppression_style != "expect-with-reason" {
        issues.push("policy.suppression_style must be expect-with-reason".to_string());
    }
    if lint_policy.policy.blanket_categories {
        issues.push("policy.blanket_categories must be false".to_string());
    }

    let ws_pkg = cargo_doc
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.as_table())
        .context("workspace.package missing")?;
    let rust_version = ws_pkg
        .get("rust-version")
        .and_then(|v| v.as_str())
        .context("workspace.package.rust-version missing")?;
    if rust_version != lint_policy.msrv {
        issues.push(format!(
            "workspace.package.rust-version {rust_version:?} != policy msrv {:?}",
            lint_policy.msrv
        ));
    }

    check_workspace_lint_inheritance(&root, &cargo_doc, &mut issues)?;
    check_active_lints_match(&cargo_doc, &lint_policy, &mut issues)?;
    check_planned_lints(rust_version, &cargo_doc, &lint_policy, &mut issues)?;
    check_clippy_toml(&root, &mut issues)?;
    check_clippy_debt(&debt_policy, &mut issues);

    finish_policy_check("check-lint-policy", issues)
}

fn check_workspace_lint_inheritance(
    root: &Path,
    cargo_doc: &toml::Value,
    issues: &mut Vec<String>,
) -> Result<()> {
    if !cargo_doc
        .get("lints")
        .and_then(|l| l.get("workspace"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        issues.push("root package must set [lints] workspace = true".to_string());
    }

    let members = cargo_doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .context("workspace.members missing")?;
    for member in members {
        let Some(path) = member.as_str() else {
            continue;
        };
        let manifest = root.join(path).join("Cargo.toml");
        if !manifest.exists() {
            issues.push(format!("{path}: Cargo.toml missing"));
            continue;
        }
        let doc = read_toml(&manifest)?;
        if !doc
            .get("lints")
            .and_then(|l| l.get("workspace"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            issues.push(format!("{path}: missing [lints] workspace = true"));
        }
    }
    Ok(())
}

fn check_active_lints_match(
    cargo_doc: &toml::Value,
    lint_policy: &ClippyLintPolicy,
    issues: &mut Vec<String>,
) -> Result<()> {
    let workspace_lints = cargo_doc
        .get("workspace")
        .and_then(|w| w.get("lints"))
        .context("workspace.lints missing")?;
    let mut cargo_active = HashMap::new();
    for table in ["rust", "clippy"] {
        let Some(lints) = workspace_lints.get(table).and_then(|v| v.as_table()) else {
            issues.push(format!("workspace.lints.{table} missing"));
            continue;
        };
        for (name, level) in lints {
            let full_name = if table == "clippy" {
                format!("clippy::{name}")
            } else {
                name.to_string()
            };
            cargo_active.insert(full_name, level.as_str().unwrap_or_default().to_string());
        }
    }

    let mut policy_active = HashMap::new();
    for lint in lint_policy
        .lint
        .iter()
        .filter(|lint| lint.status == "active")
    {
        if lint.class.trim().is_empty() {
            issues.push(format!("{}: active lint missing class", lint.name));
        }
        if lint.reason.trim().is_empty() {
            issues.push(format!("{}: active lint missing reason", lint.name));
        }
        policy_active.insert(lint.name.clone(), lint.level.clone());
    }

    for (name, level) in &cargo_active {
        if policy_active.get(name) != Some(level) {
            issues.push(format!(
                "{name}: Cargo.toml level {level:?} is not mirrored in policy/clippy-lints.toml"
            ));
        }
    }
    for name in policy_active.keys() {
        if !cargo_active.contains_key(name) {
            issues.push(format!(
                "{name}: active policy lint is missing from Cargo.toml workspace lints"
            ));
        }
    }
    Ok(())
}

fn check_planned_lints(
    rust_version: &str,
    cargo_doc: &toml::Value,
    lint_policy: &ClippyLintPolicy,
    issues: &mut Vec<String>,
) -> Result<()> {
    let workspace_lints = cargo_doc
        .get("workspace")
        .and_then(|w| w.get("lints"))
        .context("workspace.lints missing")?;
    let mut cargo_lints = HashSet::new();
    for table in ["rust", "clippy"] {
        if let Some(lints) = workspace_lints.get(table).and_then(|v| v.as_table()) {
            for name in lints.keys() {
                cargo_lints.insert(if table == "clippy" {
                    format!("clippy::{name}")
                } else {
                    name.to_string()
                });
            }
        }
    }

    for lint in lint_policy
        .lint
        .iter()
        .filter(|lint| lint.status == "planned")
    {
        let Some(activate_when_msrv) = &lint.activate_when_msrv else {
            issues.push(format!(
                "{}: planned lint missing activate_when_msrv",
                lint.name
            ));
            continue;
        };
        if lint.reason.trim().is_empty() {
            issues.push(format!("{}: planned lint missing reason", lint.name));
        }
        if semver_like_less_than(rust_version, activate_when_msrv)
            && cargo_lints.contains(&lint.name)
        {
            issues.push(format!(
                "{}: planned for MSRV {activate_when_msrv} but active at MSRV {rust_version}",
                lint.name
            ));
        }
    }
    Ok(())
}

fn check_clippy_toml(root: &Path, issues: &mut Vec<String>) -> Result<()> {
    let path = root.join("clippy.toml");
    if !path.exists() {
        issues.push("clippy.toml missing".to_string());
        return Ok(());
    }
    let text = std::fs::read_to_string(&path).context("read clippy.toml")?;
    for carveout in [
        "allow-unwrap-in-tests",
        "allow-expect-in-tests",
        "allow-panic-in-tests",
        "allow-indexing-slicing-in-tests",
        "allow-dbg-in-tests",
    ] {
        if text.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with(carveout) && trimmed.contains("true")
        }) {
            issues.push(format!("clippy.toml must not enable {carveout}"));
        }
    }
    Ok(())
}

fn check_clippy_debt(debt_policy: &ClippyDebtPolicy, issues: &mut Vec<String>) {
    for debt in &debt_policy.debt {
        require_field(&debt.lint, "clippy debt lint", issues);
        require_field(&debt.path, "clippy debt path", issues);
        require_field(&debt.owner, "clippy debt owner", issues);
        require_field(&debt.reason, "clippy debt reason", issues);
        require_field(&debt.expires, "clippy debt expires", issues);
        check_expiry(
            &debt.expires,
            &format!("clippy debt {} {}", debt.lint, debt.path),
            issues,
        );
    }
}

fn check_no_panic_family() -> Result<()> {
    let root = workspace_root()?;
    let allowlist: NoPanicAllowlist = read_toml_as(&root.join("policy/no-panic-allowlist.toml"))?;
    let mut issues = Vec::new();
    if allowlist.schema_version != "0.3" {
        issues.push("policy/no-panic-allowlist.toml schema_version must be 0.3".to_string());
    }
    for allow in &allowlist.allow {
        require_field(&allow.path, "panic allow path", &mut issues);
        require_field(&allow.family, "panic allow family", &mut issues);
        require_field(
            &allow.classification,
            "panic allow classification",
            &mut issues,
        );
        require_field(&allow.owner, "panic allow owner", &mut issues);
        require_field(&allow.explanation, "panic allow explanation", &mut issues);
        if !allow.selector.is_table() {
            issues.push(format!(
                "{} {}: selector must be a table",
                allow.path, allow.family
            ));
        }
        if let Some(last_seen) = &allow.last_seen {
            if !last_seen.is_table() {
                issues.push(format!(
                    "{} {}: last_seen must be a table",
                    allow.path, allow.family
                ));
            }
        }
        if let Some(expires) = &allow.expires {
            check_expiry(
                expires,
                &format!("panic allow {} {}", allow.path, allow.family),
                &mut issues,
            );
        }
    }
    finish_policy_check("check-no-panic-family", issues)
}

fn check_file_policy() -> Result<()> {
    let root = workspace_root()?;
    let allowlist: NonRustAllowlist = read_toml_as(&root.join("policy/non-rust-allowlist.toml"))?;
    let mut issues = Vec::new();
    if allowlist.schema_version != "1.0" {
        issues.push("policy/non-rust-allowlist.toml schema_version must be 1.0".to_string());
    }

    let mut builder = GlobSetBuilder::new();
    for allow in &allowlist.allow {
        let has_path = allow
            .path
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
        let has_glob = allow
            .glob
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
        if has_path == has_glob {
            issues.push(format!(
                "non-rust allow {:?}/{:?}: exactly one of path or glob is required",
                allow.path, allow.glob
            ));
        }
        require_field(&allow.kind, "non-rust kind", &mut issues);
        require_field(&allow.owner, "non-rust owner", &mut issues);
        require_field(&allow.reason, "non-rust reason", &mut issues);
        require_field(&allow.surface, "non-rust surface", &mut issues);
        require_field(
            &allow.classification,
            "non-rust classification",
            &mut issues,
        );
        if allow.covered_by.is_empty() {
            issues.push(format!(
                "non-rust allow {:?}/{:?}: covered_by must not be empty",
                allow.path, allow.glob
            ));
        }
        if let Some(expires) = &allow.expires {
            check_expiry(
                expires,
                &format!("non-rust allow {:?}/{:?}", allow.path, allow.glob),
                &mut issues,
            );
        }
        if let Some(path) = &allow.path {
            builder.add(Glob::new(path).with_context(|| format!("compile glob for path {path}"))?);
        }
        if let Some(glob) = &allow.glob {
            builder.add(Glob::new(glob).with_context(|| format!("compile glob {glob}"))?);
        }
    }
    let set = builder.build().context("build non-rust allow glob set")?;
    for file in walk_policy_files(&root) {
        let rel = file.strip_prefix(&root).unwrap_or(&file);
        if !set.is_match(rel) {
            issues.push(format!(
                "{}: non-Rust policy file is not covered by policy/non-rust-allowlist.toml",
                rel.display()
            ));
        }
    }
    finish_policy_check("check-file-policy", issues)
}

fn policy_report() -> Result<()> {
    let root = workspace_root()?;
    let lint_policy: ClippyLintPolicy = read_toml_as(&root.join("policy/clippy-lints.toml"))?;
    let debt_policy: ClippyDebtPolicy = read_toml_as(&root.join("policy/clippy-debt.toml"))?;
    let panic_allowlist: NoPanicAllowlist =
        read_toml_as(&root.join("policy/no-panic-allowlist.toml"))?;
    let non_rust_allowlist: NonRustAllowlist =
        read_toml_as(&root.join("policy/non-rust-allowlist.toml"))?;
    let active = lint_policy
        .lint
        .iter()
        .filter(|lint| lint.status == "active")
        .count();
    let staged = lint_policy
        .lint
        .iter()
        .filter(|lint| lint.status == "staged")
        .count();
    let planned = lint_policy
        .lint
        .iter()
        .filter(|lint| lint.status == "planned")
        .count();
    println!("lint policy: {active} active, {staged} staged, {planned} planned");
    println!("clippy debt: {} active", debt_policy.debt.len());
    println!("panic exceptions: {} active", panic_allowlist.allow.len());
    println!(
        "non-rust exceptions: {} active",
        non_rust_allowlist.allow.len()
    );
    Ok(())
}

fn walk_policy_files(root: &Path) -> Vec<PathBuf> {
    const EXTENSIONS: &[&str] = &[
        "js", "mjs", "cjs", "ts", "tsx", "jsx", "py", "sh", "yml", "yaml",
    ];
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".git" | "target")
        })
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| EXTENSIONS.contains(&extension))
        })
        .collect()
}

fn read_toml(path: &Path) -> Result<toml::Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    text.parse()
        .with_context(|| format!("parse {}", path.display()))
}

fn read_toml_as<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

fn require_field(value: &str, label: &str, issues: &mut Vec<String>) {
    if value.trim().is_empty() {
        issues.push(format!("{label} must not be empty"));
    }
}

fn check_expiry(expires: &str, label: &str, issues: &mut Vec<String>) {
    let today = Utc::now().date_naive().to_string();
    if expires < today.as_str() {
        issues.push(format!("{label} expired on {expires}"));
    }
}

fn semver_like_less_than(left: &str, right: &str) -> bool {
    fn parts(value: &str) -> Vec<u64> {
        value
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    }
    parts(left) < parts(right)
}

fn finish_policy_check(name: &str, issues: Vec<String>) -> Result<()> {
    if issues.is_empty() {
        eprintln!("{name} passed ✓");
        return Ok(());
    }
    for issue in &issues {
        eprintln!("  ✗ {issue}");
    }
    anyhow::bail!("{name} found {} issue(s)", issues.len())
}

// ── setup ────────────────────────────────────────────────────────────

fn setup() -> Result<()> {
    let status = Cmd::new("git")
        .args(["config", "core.hooksPath", ".githooks"])
        .status()
        .context("run git config")?;
    anyhow::ensure!(status.success(), "git config failed ({})", status);

    // Best-effort chmod on Unix (no-op failure on Windows)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for hook in [".githooks/pre-commit", ".githooks/pre-push"] {
            if let Ok(meta) = std::fs::metadata(hook) {
                let mut perms = meta.permissions();
                perms.set_mode(perms.mode() | 0o111);
                let _ = std::fs::set_permissions(hook, perms);
            }
        }
    }

    eprintln!("hooks installed: core.hooksPath = .githooks");
    Ok(())
}

fn warn_if_hooks_missing() {
    // Don't warn in CI — hooks are irrelevant there
    if std::env::var_os("CI").is_some() || std::env::var_os("GITHUB_ACTIONS").is_some() {
        return;
    }
    let output = Cmd::new("git")
        .args(["config", "--get", "core.hooksPath"])
        .output();
    let installed = output
        .as_ref()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| std::str::from_utf8(&o.stdout).ok())
        .is_some_and(|s| s.trim() == ".githooks");
    if !installed {
        eprintln!(
            "warning: git hooks not installed. Run `cargo xtask setup` to enable pre-commit checks."
        );
    }
}
