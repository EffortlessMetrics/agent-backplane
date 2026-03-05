// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt export and import in JSON, JSONL, CSV, and summary report formats.

use abp_core::Receipt;

use crate::Result;
use crate::error::StoreError;
use crate::stats::ReceiptStats;

/// Export receipts as a single JSON array.
pub fn export_json(receipts: &[Receipt]) -> Result<String> {
    serde_json::to_string_pretty(receipts).map_err(StoreError::from)
}

/// Export receipts in JSON-lines format (one JSON object per line).
pub fn export_jsonl(receipts: &[Receipt]) -> Result<String> {
    let mut buf = String::new();
    for r in receipts {
        let line = serde_json::to_string(r)?;
        buf.push_str(&line);
        buf.push('\n');
    }
    Ok(buf)
}

/// Export receipts as CSV with header row.
///
/// Columns: `run_id,work_order_id,backend,outcome,started_at,finished_at,duration_ms,input_tokens,output_tokens`
pub fn export_csv(receipts: &[Receipt]) -> Result<String> {
    let mut buf = String::from(
        "run_id,work_order_id,backend,outcome,started_at,finished_at,duration_ms,input_tokens,output_tokens\n",
    );
    for r in receipts {
        let input = r
            .usage
            .input_tokens
            .map_or(String::new(), |t| t.to_string());
        let output = r
            .usage
            .output_tokens
            .map_or(String::new(), |t| t.to_string());
        buf.push_str(&format!(
            "{},{},{},{:?},{},{},{},{},{}\n",
            r.meta.run_id,
            r.meta.work_order_id,
            r.backend.id,
            r.outcome,
            r.meta.started_at.to_rfc3339(),
            r.meta.finished_at.to_rfc3339(),
            r.meta.duration_ms,
            input,
            output,
        ));
    }
    Ok(buf)
}

/// Export a human-readable summary report from a set of receipts.
pub fn export_summary(receipts: &[Receipt]) -> String {
    if receipts.is_empty() {
        return "No receipts to summarize.\n".to_string();
    }
    let stats = ReceiptStats::from_receipts(receipts);
    let mut buf = String::new();
    buf.push_str(&format!("Receipt Summary ({} total)\n", stats.total));
    buf.push_str(&format!("{}\n", "=".repeat(40)));

    if let Some(rate) = stats.success_rate {
        buf.push_str(&format!("Success rate: {:.1}%\n", rate * 100.0));
    }
    if let Some(avg) = stats.avg_duration_ms {
        buf.push_str(&format!("Avg duration: {:.1} ms\n", avg));
    }
    if let Some(min) = stats.min_duration_ms {
        buf.push_str(&format!("Min duration: {} ms\n", min));
    }
    if let Some(max) = stats.max_duration_ms {
        buf.push_str(&format!("Max duration: {} ms\n", max));
    }
    buf.push_str(&format!(
        "Total tokens: {} in / {} out\n",
        stats.total_input_tokens, stats.total_output_tokens
    ));

    if !stats.by_outcome.is_empty() {
        buf.push_str("\nBy outcome:\n");
        for (outcome, count) in &stats.by_outcome {
            buf.push_str(&format!("  {outcome}: {count}\n"));
        }
    }
    if !stats.by_backend.is_empty() {
        buf.push_str("\nBy backend:\n");
        for (backend, count) in &stats.by_backend {
            buf.push_str(&format!("  {backend}: {count}\n"));
        }
    }
    buf
}

/// Import receipts from a JSON array string.
pub fn import_json(data: &str) -> Result<Vec<Receipt>> {
    serde_json::from_str(data).map_err(StoreError::from)
}

/// Import receipts from a JSONL string (one JSON object per line).
pub fn import_jsonl(data: &str) -> Result<Vec<Receipt>> {
    let mut receipts = Vec::new();
    for (i, line) in data.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let receipt: Receipt =
            serde_json::from_str(line).map_err(|e| StoreError::Other(format!("line {i}: {e}")))?;
        receipts.push(receipt);
    }
    Ok(receipts)
}
