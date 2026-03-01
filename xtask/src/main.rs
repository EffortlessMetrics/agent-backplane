// SPDX-License-Identifier: MIT OR Apache-2.0
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use schemars::schema_for;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Schema { out_dir } => schema(out_dir),
        Command::Check => check(),
        Command::Coverage => coverage(),
        Command::Lint => lint(),
        Command::ReleaseCheck => release_check(),
        Command::Docs { open } => docs(open),
        Command::ListCrates => list_crates(),
        Command::Audit => audit(),
        Command::Stats => stats(),
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

fn check() -> Result<()> {
    let steps: &[(&str, &[&str])] = &[
        ("fmt", &["fmt", "--all", "--", "--check"]),
        (
            "clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("test", &["test", "--workspace"]),
        ("doc-test", &["test", "--doc", "--workspace"]),
    ];

    let mut results: Vec<(&str, bool)> = Vec::new();
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
    run_cargo(&["fmt", "--all", "--", "--check"])?;
    run_cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ])?;
    eprintln!("lint passed ✓");
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

        // Check required fields (may be inherited via `.workspace = true`)
        for field in ["version", "edition", "license"] {
            if pkg.and_then(|p| p.get(field)).is_none() {
                eprintln!("  ✗ {name}: missing package.{field}");
                ok = false;
            }
        }

        // Check README exists
        let readme_path = root.join(path).join("README.md");
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
        eprintln!("  ✓ all crates have required fields, README, and consistent versions");
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
