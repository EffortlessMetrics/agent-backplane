// SPDX-License-Identifier: MIT OR Apache-2.0
//! Snapshot tests for abp-integrations types and projection matrix.

use abp_core::*;
use abp_integrations::projection::{Dialect, ProjectionMatrix};
use abp_integrations::{Backend, MockBackend};
use insta::{assert_json_snapshot, assert_snapshot};
use tokio::sync::mpsc;
use uuid::Uuid;

// ── 1. ProjectionMatrix supported translations ─────────────────────────

#[test]
fn snapshot_supported_translations() {
    let matrix = ProjectionMatrix::new();
    let pairs = matrix.supported_translations();
    let formatted: Vec<String> = pairs
        .iter()
        .map(|(from, to)| format!("{from:?} -> {to:?}"))
        .collect();
    assert_snapshot!("integrations_supported_translations", formatted.join("\n"));
}

// ── 2. MockBackend identity ─────────────────────────────────────────────

#[test]
fn snapshot_mock_backend_identity() {
    let backend = MockBackend;
    let identity = backend.identity();
    assert_json_snapshot!("integrations_mock_identity", identity);
}

// ── 3. MockBackend capabilities ─────────────────────────────────────────

#[test]
fn snapshot_mock_backend_capabilities() {
    let backend = MockBackend;
    let caps = backend.capabilities();
    let value = serde_json::to_value(&caps).unwrap();
    assert_json_snapshot!("integrations_mock_capabilities", value);
}

// ── 4. MockBackend receipt ──────────────────────────────────────────────

#[tokio::test]
async fn snapshot_mock_backend_receipt() {
    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("snapshot test task")
        .root("/tmp/test")
        .build();
    let run_id = Uuid::nil();
    let (tx, _rx) = mpsc::channel(32);

    let receipt = backend.run(run_id, wo, tx).await.unwrap();
    let value = serde_json::to_value(&receipt).unwrap();

    assert_json_snapshot!("integrations_mock_receipt", value, {
        ".meta.started_at" => "[timestamp]",
        ".meta.finished_at" => "[timestamp]",
        ".meta.duration_ms" => "[duration]",
        ".meta.work_order_id" => "[uuid]",
        ".receipt_sha256" => "[sha256]",
        ".trace[].ts" => "[timestamp]"
    });
}

// ── 5. Dialect enum variants serialization ──────────────────────────────

#[test]
fn snapshot_dialect_variants() {
    let dialects: Vec<serde_json::Value> = Dialect::ALL
        .iter()
        .map(|d| serde_json::to_value(d).unwrap())
        .collect();
    assert_json_snapshot!("integrations_dialect_variants", dialects);
}

// ── 6. ABP-to-Claude translation ────────────────────────────────────────

#[test]
fn snapshot_abp_to_claude_translation() {
    let matrix = ProjectionMatrix::new();
    let wo = WorkOrderBuilder::new("Refactor the auth module")
        .model("claude-sonnet-4-20250514")
        .build();
    let result = matrix
        .translate(Dialect::Abp, Dialect::Claude, &wo)
        .unwrap();
    assert_json_snapshot!("integrations_abp_to_claude", result);
}

// ── 7. ABP-to-Codex translation ────────────────────────────────────────

#[test]
fn snapshot_abp_to_codex_translation() {
    let matrix = ProjectionMatrix::new();
    let wo = WorkOrderBuilder::new("Fix the login bug")
        .model("codex-mini-latest")
        .build();
    let result = matrix.translate(Dialect::Abp, Dialect::Codex, &wo).unwrap();
    assert_json_snapshot!("integrations_abp_to_codex", result);
}

// ── 8. ABP-to-Gemini translation ───────────────────────────────────────

#[test]
fn snapshot_abp_to_gemini_translation() {
    let matrix = ProjectionMatrix::new();
    let wo = WorkOrderBuilder::new("Generate unit tests")
        .model("gemini-2.5-flash")
        .build();
    let result = matrix
        .translate(Dialect::Abp, Dialect::Gemini, &wo)
        .unwrap();
    assert_json_snapshot!("integrations_abp_to_gemini", result);
}

// ── 9. ABP-to-Kimi translation ─────────────────────────────────────────

#[test]
fn snapshot_abp_to_kimi_translation() {
    let matrix = ProjectionMatrix::new();
    let wo = WorkOrderBuilder::new("Translate docs to Chinese")
        .model("moonshot-v1-8k")
        .build();
    let result = matrix.translate(Dialect::Abp, Dialect::Kimi, &wo).unwrap();
    assert_json_snapshot!("integrations_abp_to_kimi", result);
}
