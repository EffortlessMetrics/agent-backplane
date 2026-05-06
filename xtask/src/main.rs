// SPDX-License-Identifier: MIT OR Apache-2.0
use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use clap::{Parser, Subcommand};
use schemars::schema_for;
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
    /// Check Clippy policy files, workspace lint inheritance, and debt metadata.
    CheckLintPolicy,
    /// Summarize policy debt and planned lint flips.
    PolicyReport,
    /// Configure local repo for development (install git hooks).
    Setup,
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
        Command::CheckLintPolicy => check_lint_policy(),
        Command::PolicyReport => policy_report(),
        Command::Setup => setup(),
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

    let policy_ok = check_lint_policy().is_ok();
    let mut results: Vec<(&str, bool)> = vec![("fmt", fmt_ok), ("lint-policy", policy_ok)];
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
    check_lint_policy()?;
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

// ── lint policy ──────────────────────────────────────────────────────

fn check_lint_policy() -> Result<()> {
    let root = workspace_root()?;
    let root_manifest = read_toml(&root.join("Cargo.toml"))?;
    let policy = read_toml(&root.join("policy/clippy-lints.toml"))?;
    let debt = read_toml(&root.join("policy/clippy-debt.toml"))?;
    let no_panic = read_toml(&root.join("policy/no-panic-allowlist.toml"))?;
    let non_rust = read_toml(&root.join("policy/non-rust-allowlist.toml"))?;

    let mut issues = 0u32;

    let policy_msrv = required_str(&policy, &["msrv"], "policy/clippy-lints.toml msrv")?;
    let workspace_msrv = required_str(
        &root_manifest,
        &["workspace", "package", "rust-version"],
        "workspace.package.rust-version",
    )?;
    if workspace_msrv != policy_msrv {
        eprintln!(
            "  ✗ workspace.package.rust-version ({workspace_msrv}) does not match policy msrv ({policy_msrv})"
        );
        issues += 1;
    }

    let policy_table = required_table(&policy, &["policy"], "policy table")?;
    for (key, expected) in [
        ("panic_free_tests", true),
        ("allow_test_carveouts", false),
        ("blanket_categories", false),
    ] {
        if policy_table.get(key).and_then(toml::Value::as_bool) != Some(expected) {
            eprintln!("  ✗ policy.{key} must be {expected}");
            issues += 1;
        }
    }
    if policy_table
        .get("suppression_style")
        .and_then(toml::Value::as_str)
        != Some("expect-with-reason")
    {
        eprintln!("  ✗ policy.suppression_style must be expect-with-reason");
        issues += 1;
    }

    if required_table(&root_manifest, &["workspace", "lints"], "workspace.lints").is_err() {
        eprintln!("  ✗ root Cargo.toml must define [workspace.lints]");
        issues += 1;
    }

    let members = root_manifest
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
        .context("workspace.members not found")?;
    for manifest in std::iter::once(root.join("Cargo.toml")).chain(
        members
            .iter()
            .filter_map(toml::Value::as_str)
            .map(|member| root.join(member).join("Cargo.toml")),
    ) {
        let display = manifest.strip_prefix(&root).unwrap_or(&manifest).display();
        let doc = read_toml(&manifest)?;
        if doc
            .get("lints")
            .and_then(|l| l.get("workspace"))
            .and_then(toml::Value::as_bool)
            != Some(true)
        {
            eprintln!("  ✗ {display}: missing [lints] workspace = true");
            issues += 1;
        }
    }

    issues += check_lint_entries(&policy)?;
    issues += check_planned_lints(&policy, policy_msrv)?;
    issues += check_clippy_toml(&root)?;
    issues += check_debt_entries(&debt)?;
    issues += check_no_panic_allowlist(&no_panic)?;
    issues += check_non_rust_allowlist(&non_rust)?;

    if issues == 0 {
        eprintln!("lint policy passed ✓");
        Ok(())
    } else {
        anyhow::bail!("lint policy found {issues} issue(s)");
    }
}

fn policy_report() -> Result<()> {
    let root = workspace_root()?;
    let policy = read_toml(&root.join("policy/clippy-lints.toml"))?;
    let debt = read_toml(&root.join("policy/clippy-debt.toml"))?;
    let no_panic = read_toml(&root.join("policy/no-panic-allowlist.toml"))?;
    let non_rust = read_toml(&root.join("policy/non-rust-allowlist.toml"))?;

    let active = policy_array(&policy, "lint")?
        .iter()
        .filter(|lint| lint.get("status").and_then(toml::Value::as_str) == Some("active"))
        .count();
    let debt_tracked = policy_array(&policy, "lint")?
        .iter()
        .filter(|lint| lint.get("status").and_then(toml::Value::as_str) == Some("debt-tracked"))
        .count();
    let planned = policy_array(&policy, "planned")?.len();
    let debt_count = policy_array(&debt, "debt")?.len();
    let panic_exceptions = optional_policy_array(&no_panic, "allow").len();
    let non_rust_exceptions = optional_policy_array(&non_rust, "allow").len();

    println!("lint policy report");
    println!("  active lints:       {active}");
    println!("  debt-tracked lints: {debt_tracked}");
    println!("  planned flips:      {planned}");
    println!("  debt entries:       {debt_count}");
    println!("  panic exceptions:   {panic_exceptions}");
    println!("  non-Rust entries:   {non_rust_exceptions}");
    Ok(())
}

fn read_toml(path: &Path) -> Result<toml::Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    text.parse()
        .with_context(|| format!("parse {}", path.display()))
}

fn required_table<'a>(
    doc: &'a toml::Value,
    path: &[&str],
    label: &str,
) -> Result<&'a toml::map::Map<String, toml::Value>> {
    let mut current = doc;
    for key in path {
        current = current
            .get(*key)
            .with_context(|| format!("{label} missing"))?;
    }
    current
        .as_table()
        .with_context(|| format!("{label} must be a table"))
}

fn required_str<'a>(doc: &'a toml::Value, path: &[&str], label: &str) -> Result<&'a str> {
    let mut current = doc;
    for key in path {
        current = current
            .get(*key)
            .with_context(|| format!("{label} missing"))?;
    }
    current
        .as_str()
        .with_context(|| format!("{label} must be a string"))
}

fn optional_policy_array<'a>(doc: &'a toml::Value, key: &str) -> Vec<&'a toml::Value> {
    doc.get(key)
        .and_then(toml::Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn policy_array<'a>(doc: &'a toml::Value, key: &str) -> Result<&'a Vec<toml::Value>> {
    doc.get(key)
        .and_then(toml::Value::as_array)
        .with_context(|| format!("policy array [[{key}]] missing"))
}

fn check_lint_entries(policy: &toml::Value) -> Result<u32> {
    let mut issues = 0;
    let mut names = HashSet::new();
    for lint in policy_array(policy, "lint")? {
        for field in ["name", "level", "status", "class", "reason"] {
            if lint
                .get(field)
                .and_then(toml::Value::as_str)
                .is_none_or(str::is_empty)
            {
                eprintln!("  ✗ [[lint]] entry missing non-empty {field}");
                issues += 1;
            }
        }
        if let Some(name) = lint.get("name").and_then(toml::Value::as_str) {
            if !names.insert(name.to_string()) {
                eprintln!("  ✗ duplicate lint entry for {name}");
                issues += 1;
            }
        }
    }
    Ok(issues)
}

fn check_planned_lints(policy: &toml::Value, policy_msrv: &str) -> Result<u32> {
    let mut issues = 0;
    let mut seen_194 = false;
    let mut seen_195 = false;
    for planned in policy_array(policy, "planned")? {
        for field in ["name", "level", "activate_when_msrv", "reason"] {
            if planned
                .get(field)
                .and_then(toml::Value::as_str)
                .is_none_or(str::is_empty)
            {
                eprintln!("  ✗ [[planned]] entry missing non-empty {field}");
                issues += 1;
            }
        }
        if let Some(msrv) = planned
            .get("activate_when_msrv")
            .and_then(toml::Value::as_str)
        {
            seen_194 |= msrv == "1.94";
            seen_195 |= msrv == "1.95";
            if msrv <= policy_msrv {
                eprintln!(
                    "  ✗ planned lint activates at {msrv}, which is not after policy MSRV {policy_msrv}"
                );
                issues += 1;
            }
        }
    }
    if !seen_194 || !seen_195 {
        eprintln!("  ✗ planned lint ledger must include both Rust 1.94 and 1.95 flips");
        issues += 1;
    }
    Ok(issues)
}

fn check_clippy_toml(root: &Path) -> Result<u32> {
    let path = root.join("clippy.toml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut issues = 0;
    for carveout in [
        "allow-unwrap-in-tests",
        "allow-expect-in-tests",
        "allow-panic-in-tests",
        "allow-indexing-slicing-in-tests",
        "allow-dbg-in-tests",
    ] {
        if text.lines().any(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#') && trimmed.starts_with(carveout)
        }) {
            eprintln!("  ✗ clippy.toml must not set {carveout}");
            issues += 1;
        }
    }
    Ok(issues)
}

fn check_no_panic_allowlist(doc: &toml::Value) -> Result<u32> {
    let mut issues = 0;
    if doc.get("schema_version").and_then(toml::Value::as_str) != Some("0.3") {
        eprintln!("  ✗ policy/no-panic-allowlist.toml schema_version must be 0.3");
        issues += 1;
    }
    let today = Utc::now().date_naive();
    for entry in optional_policy_array(doc, "allow") {
        for field in ["path", "family", "classification", "owner", "explanation"] {
            if entry
                .get(field)
                .and_then(toml::Value::as_str)
                .is_none_or(str::is_empty)
            {
                eprintln!("  ✗ [[allow]] panic entry missing non-empty {field}");
                issues += 1;
            }
        }
        if entry
            .get("selector")
            .and_then(toml::Value::as_table)
            .is_none()
        {
            eprintln!("  ✗ [[allow]] panic entry missing [allow.selector]");
            issues += 1;
        }
        if let Some(expires) = entry.get("expires").and_then(toml::Value::as_str) {
            match NaiveDate::parse_from_str(expires, "%Y-%m-%d") {
                Ok(date) if date < today => {
                    eprintln!("  ✗ [[allow]] panic entry expired on {expires}");
                    issues += 1;
                }
                Ok(_) => {}
                Err(_) => {
                    eprintln!("  ✗ [[allow]] panic expires must be YYYY-MM-DD: {expires}");
                    issues += 1;
                }
            }
        }
    }
    Ok(issues)
}

fn check_non_rust_allowlist(doc: &toml::Value) -> Result<u32> {
    let mut issues = 0;
    if doc.get("schema_version").and_then(toml::Value::as_str) != Some("1.0") {
        eprintln!("  ✗ policy/non-rust-allowlist.toml schema_version must be 1.0");
        issues += 1;
    }
    let today = Utc::now().date_naive();
    for entry in optional_policy_array(doc, "allow") {
        let has_locator = entry.get("path").and_then(toml::Value::as_str).is_some()
            || entry.get("glob").and_then(toml::Value::as_str).is_some();
        if !has_locator {
            eprintln!("  ✗ [[allow]] non-Rust entry must set path or glob");
            issues += 1;
        }
        for field in ["kind", "owner", "reason", "surface", "classification"] {
            if entry
                .get(field)
                .and_then(toml::Value::as_str)
                .is_none_or(str::is_empty)
            {
                eprintln!("  ✗ [[allow]] non-Rust entry missing non-empty {field}");
                issues += 1;
            }
        }
        if entry
            .get("covered_by")
            .and_then(toml::Value::as_array)
            .is_none_or(Vec::is_empty)
        {
            eprintln!("  ✗ [[allow]] non-Rust entry missing covered_by commands");
            issues += 1;
        }
        if let Some(expires) = entry.get("expires").and_then(toml::Value::as_str) {
            match NaiveDate::parse_from_str(expires, "%Y-%m-%d") {
                Ok(date) if date < today => {
                    eprintln!("  ✗ [[allow]] non-Rust entry expired on {expires}");
                    issues += 1;
                }
                Ok(_) => {}
                Err(_) => {
                    eprintln!("  ✗ [[allow]] non-Rust expires must be YYYY-MM-DD: {expires}");
                    issues += 1;
                }
            }
        }
    }
    Ok(issues)
}

fn check_debt_entries(debt: &toml::Value) -> Result<u32> {
    let mut issues = 0;
    let today = Utc::now().date_naive();
    for entry in policy_array(debt, "debt")? {
        for field in ["lint", "path", "owner", "reason", "expires"] {
            if entry
                .get(field)
                .and_then(toml::Value::as_str)
                .is_none_or(str::is_empty)
            {
                eprintln!("  ✗ [[debt]] entry missing non-empty {field}");
                issues += 1;
            }
        }
        if let Some(expires) = entry.get("expires").and_then(toml::Value::as_str) {
            match NaiveDate::parse_from_str(expires, "%Y-%m-%d") {
                Ok(date) if date < today => {
                    eprintln!("  ✗ [[debt]] entry expired on {expires}");
                    issues += 1;
                }
                Ok(_) => {}
                Err(_) => {
                    eprintln!("  ✗ [[debt]] expires must be YYYY-MM-DD: {expires}");
                    issues += 1;
                }
            }
        }
    }
    Ok(issues)
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
