// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt export in multiple formats for bulk reporting.

use abp_core::{ContractError, Receipt};

/// Serialize a collection of receipts as a JSON array (pretty-printed).
///
/// # Errors
///
/// Returns [`ContractError::Json`] if serialization fails.
pub fn to_json(receipts: &[Receipt]) -> Result<String, ContractError> {
    Ok(serde_json::to_string_pretty(receipts)?)
}

/// Serialize a collection of receipts as JSONL (one compact JSON object per line).
///
/// # Errors
///
/// Returns [`ContractError::Json`] if serialization fails.
pub fn to_jsonl(receipts: &[Receipt]) -> Result<String, ContractError> {
    let mut out = String::new();
    for r in receipts {
        let line = serde_json::to_string(r)?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

/// Serialize a collection of receipts as CSV.
///
/// Columns: `run_id`, `work_order_id`, `backend`, `outcome`, `started_at`,
/// `finished_at`, `duration_ms`, `input_tokens`, `output_tokens`, `hash`.
pub fn to_csv(receipts: &[Receipt]) -> String {
    let mut out = String::from(
        "run_id,work_order_id,backend,outcome,started_at,finished_at,duration_ms,input_tokens,output_tokens,hash\n",
    );
    for r in receipts {
        out.push_str(&csv_escape(&r.meta.run_id.to_string()));
        out.push(',');
        out.push_str(&csv_escape(&r.meta.work_order_id.to_string()));
        out.push(',');
        out.push_str(&csv_escape(&r.backend.id));
        out.push(',');
        out.push_str(&csv_escape(&format!("{:?}", r.outcome)));
        out.push(',');
        out.push_str(&csv_escape(&r.meta.started_at.to_rfc3339()));
        out.push(',');
        out.push_str(&csv_escape(&r.meta.finished_at.to_rfc3339()));
        out.push(',');
        out.push_str(&r.meta.duration_ms.to_string());
        out.push(',');
        out.push_str(&opt_u64(r.usage.input_tokens));
        out.push(',');
        out.push_str(&opt_u64(r.usage.output_tokens));
        out.push(',');
        out.push_str(&csv_escape(r.receipt_sha256.as_deref().unwrap_or("")));
        out.push('\n');
    }
    out
}

/// Produce a human-readable summary table suitable for terminal output.
pub fn to_summary_table(receipts: &[Receipt]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:<36}  {:<10}  {:>10}  {:>8}  {:>8}  {}\n",
        "RUN_ID", "OUTCOME", "DURATION", "TOK_IN", "TOK_OUT", "BACKEND"
    ));
    out.push_str(&"-".repeat(96));
    out.push('\n');

    for r in receipts {
        let run_id = r.meta.run_id.to_string();
        let outcome = format!("{:?}", r.outcome);
        let duration = format!("{}ms", r.meta.duration_ms);
        let tok_in = opt_u64(r.usage.input_tokens);
        let tok_out = opt_u64(r.usage.output_tokens);
        let backend = &r.backend.id;

        out.push_str(&format!(
            "{:<36}  {:<10}  {:>10}  {:>8}  {:>8}  {}\n",
            run_id, outcome, duration, tok_in, tok_out, backend
        ));
    }

    let total = receipts.len();
    let complete = receipts
        .iter()
        .filter(|r| r.outcome == abp_core::Outcome::Complete)
        .count();
    out.push_str(&"-".repeat(96));
    out.push_str(&format!("\n{total} receipts, {complete} complete"));
    out
}

fn opt_u64(v: Option<u64>) -> String {
    match v {
        Some(n) => n.to_string(),
        None => "-".to_string(),
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}
