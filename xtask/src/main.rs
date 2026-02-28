// SPDX-License-Identifier: MIT OR Apache-2.0
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use schemars::schema_for;
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
    /// Run full CI checks locally (fmt, clippy, test, doc).
    Check,
    /// Print instructions for running code coverage with tarpaulin.
    Coverage,
    /// List all workspace crates with their paths.
    ListCrates,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Schema { out_dir } => schema(out_dir),
        Command::Check => check(),
        Command::Coverage => coverage(),
        Command::ListCrates => list_crates(),
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
    anyhow::ensure!(status.success(), "cargo {} failed ({})", args.join(" "), status);
    Ok(())
}

fn check() -> Result<()> {
    run_cargo(&["fmt", "--all", "--", "--check"])?;
    run_cargo(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    run_cargo(&["test", "--workspace"])?;
    run_cargo(&["doc", "--workspace", "--no-deps"])?;
    eprintln!("all checks passed ✓");
    Ok(())
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
    xtask_dir.parent().map(PathBuf::from).context("find workspace root")
}

fn read_crate_name(path: &PathBuf) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let doc: toml::Value = content.parse().ok()?;
    doc.get("package")?.get("name")?.as_str().map(String::from)
}
