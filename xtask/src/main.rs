// SPDX-License-Identifier: MIT OR Apache-2.0
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use schemars::schema_for;
use std::path::PathBuf;

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Schema { out_dir } => schema(out_dir),
    }
}

fn schema(out_dir: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&out_dir).context("create schema output dir")?;

    let wo = schema_for!(abp_core::WorkOrder);
    let receipt = schema_for!(abp_core::Receipt);

    write_schema(&out_dir.join("work_order.schema.json"), &wo)?;
    write_schema(&out_dir.join("receipt.schema.json"), &receipt)?;

    eprintln!("wrote schemas to {}", out_dir.display());
    Ok(())
}

fn write_schema(path: &PathBuf, schema: &schemars::Schema) -> Result<()> {
    let s = serde_json::to_string_pretty(schema)?;
    std::fs::write(path, s).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
