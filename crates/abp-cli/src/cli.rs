// SPDX-License-Identifier: MIT OR Apache-2.0
//! CLI argument definitions for the Agent Backplane.
//!
//! These types are extracted from the binary so that integration tests
//! can exercise argument parsing via [`clap::Parser::try_parse_from`]
//! without spawning a process.

use abp_core::{ExecutionLane, WorkspaceMode};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Top-level CLI argument parser.
#[derive(Parser, Debug)]
#[command(name = "abp", version, about = "Agent Backplane CLI")]
pub struct Cli {
    /// The subcommand to execute.
    #[command(subcommand)]
    pub command: Commands,

    /// Enable debug logging.
    #[arg(long)]
    pub debug: bool,

    /// Path to a TOML configuration file.
    ///
    /// Falls back to `backplane.toml` in the current directory if present.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
}

/// Available CLI subcommands.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List available backends.
    Backends,

    /// Run a work order.
    Run {
        /// Backend name: mock | sidecar:node | sidecar:python | sidecar:claude | sidecar:copilot | sidecar:kimi | sidecar:gemini | sidecar:codex.
        /// Aliases are also supported: node, python, claude, copilot, kimi, gemini, codex.
        #[arg(long)]
        backend: Option<String>,

        /// Task to execute.
        #[arg(long)]
        task: String,

        /// Preferred model (sets work_order.config.model).
        #[arg(long)]
        model: Option<String>,

        /// Workspace root.
        #[arg(long, default_value = ".")]
        root: String,

        /// Workspace mode (pass-through or staged).
        #[arg(long, value_enum, default_value_t = WorkspaceModeArg::Staged)]
        workspace_mode: WorkspaceModeArg,

        /// Execution lane.
        #[arg(long, value_enum, default_value_t = LaneArg::PatchFirst)]
        lane: LaneArg,

        /// Include glob(s) (relative to root). Can be repeated.
        #[arg(long)]
        include: Vec<String>,

        /// Exclude glob(s) (relative to root). Can be repeated.
        #[arg(long)]
        exclude: Vec<String>,

        /// Vendor params as key=value. Repeated values are merged.
        ///
        /// Examples:
        /// --param model=gemini-2.5-flash
        /// --param abp.mode=passthrough
        /// --param stream=true
        #[arg(long = "param")]
        params: Vec<String>,

        /// Environment variables passed through to the runtime as KEY=VALUE.
        #[arg(long = "env")]
        env_vars: Vec<String>,

        /// Optional hard cap on run budget in USD (best-effort).
        #[arg(long)]
        max_budget_usd: Option<f64>,

        /// Optional hard cap on run turns/iterations (best-effort).
        #[arg(long)]
        max_turns: Option<u32>,

        /// Where to write the receipt (defaults to .agent-backplane/receipts/<run_id>.json).
        #[arg(long)]
        out: Option<PathBuf>,

        /// Print JSON instead of pretty output.
        #[arg(long)]
        json: bool,

        /// Path to a policy profile JSON file to load.
        #[arg(long)]
        policy: Option<PathBuf>,

        /// Write the receipt to this file path.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Write streamed events as JSONL to this file.
        #[arg(long)]
        events: Option<PathBuf>,
    },

    /// Validate a JSON file as a WorkOrder, Receipt, or auto-detect type.
    Validate {
        /// Path to the JSON file.
        #[arg()]
        file: PathBuf,
    },

    /// Print a JSON schema to stdout.
    Schema {
        /// Which schema to print.
        #[arg(value_enum)]
        kind: SchemaArg,
    },

    /// Inspect a receipt file and verify its hash.
    Inspect {
        /// Path to the receipt JSON file.
        #[arg()]
        file: PathBuf,
    },

    /// Load and validate configuration.
    #[command(name = "config")]
    ConfigCmd {
        /// The config action to perform.
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Receipt inspection and comparison.
    #[command(name = "receipt")]
    ReceiptCmd {
        /// The receipt action to perform.
        #[command(subcommand)]
        action: ReceiptAction,
    },
}

/// Actions for the `config` subcommand.
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Check (load and validate) the configuration file.
    Check {
        /// Path to a TOML configuration file (overrides --config).
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

/// Actions for the `receipt` subcommand.
#[derive(Subcommand, Debug)]
pub enum ReceiptAction {
    /// Verify a receipt file's hash integrity.
    Verify {
        /// Path to the receipt JSON file.
        #[arg()]
        file: PathBuf,
    },
    /// Diff two receipt files and show changes.
    Diff {
        /// First receipt JSON file.
        #[arg()]
        file1: PathBuf,
        /// Second receipt JSON file.
        #[arg()]
        file2: PathBuf,
    },
}

/// Schema kind argument for the `schema` subcommand.
#[derive(Debug, Clone, ValueEnum)]
pub enum SchemaArg {
    /// WorkOrder schema.
    WorkOrder,
    /// Receipt schema.
    Receipt,
    /// BackplaneConfig schema.
    Config,
}

/// Workspace mode CLI argument.
#[derive(Debug, Clone, ValueEnum)]
pub enum WorkspaceModeArg {
    /// Pass through workspace unchanged.
    PassThrough,
    /// Stage workspace in a temp directory.
    Staged,
}

impl From<WorkspaceModeArg> for WorkspaceMode {
    fn from(v: WorkspaceModeArg) -> Self {
        match v {
            WorkspaceModeArg::PassThrough => WorkspaceMode::PassThrough,
            WorkspaceModeArg::Staged => WorkspaceMode::Staged,
        }
    }
}

/// Execution lane CLI argument.
#[derive(Debug, Clone, ValueEnum)]
pub enum LaneArg {
    /// Patch-first execution lane.
    PatchFirst,
    /// Workspace-first execution lane.
    WorkspaceFirst,
}

impl From<LaneArg> for ExecutionLane {
    fn from(v: LaneArg) -> Self {
        match v {
            LaneArg::PatchFirst => ExecutionLane::PatchFirst,
            LaneArg::WorkspaceFirst => ExecutionLane::WorkspaceFirst,
        }
    }
}
