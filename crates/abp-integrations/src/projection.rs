// SPDX-License-Identifier: MIT OR Apache-2.0
//! Projection matrix for translating between vendor dialects.
//!
//! In v0.1, the matrix supports:
//! - **Identity translations**: same dialect in and out (pass-through).
//! - **ABP-to-vendor translations**: convert an ABP [`WorkOrder`] into the
//!   vendor-specific request JSON for each supported dialect.

use abp_core::WorkOrder;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Identifies a vendor dialect for translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    /// The canonical ABP contract format.
    Abp,
    /// Anthropic Claude Messages API.
    Claude,
    /// OpenAI Codex / Responses API.
    Codex,
    /// Google Gemini generateContent API.
    Gemini,
    /// Moonshot Kimi chat completions API.
    Kimi,
}

impl Dialect {
    /// All known dialect variants.
    pub const ALL: &[Dialect] = &[
        Dialect::Abp,
        Dialect::Claude,
        Dialect::Codex,
        Dialect::Gemini,
        Dialect::Kimi,
    ];
}

/// Routes translations between vendor dialects.
///
/// The projection matrix knows which `(source, target)` dialect pairs are
/// valid and performs the mapping via inline translation logic that mirrors
/// each SDK adapter's `map_work_order` function.
///
/// In v0.1 the supported translations are:
/// - Identity (any dialect to itself)
/// - ABP → Claude / Codex / Gemini / Kimi
#[derive(Debug, Clone, Default)]
pub struct ProjectionMatrix;

impl ProjectionMatrix {
    /// Create a new projection matrix.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Translate a [`WorkOrder`] from one dialect to another.
    ///
    /// Identity translations serialise the work order as-is.
    /// ABP-to-vendor translations build the vendor request JSON using
    /// the same logic as each SDK adapter's `map_work_order`.
    ///
    /// # Errors
    ///
    /// Returns an error when the `(from, to)` pair is not a supported
    /// translation in the current version of the matrix.
    pub fn translate(
        &self,
        from: Dialect,
        to: Dialect,
        wo: &WorkOrder,
    ) -> Result<serde_json::Value> {
        translate(from, to, wo)
    }

    /// List all translation pairs the matrix currently supports.
    #[must_use]
    pub fn supported_translations(&self) -> Vec<(Dialect, Dialect)> {
        supported_translations()
    }
}

// ---------------------------------------------------------------------------
// Inline translation helpers
//
// These mirror the SDK adapter `map_work_order` functions but produce
// `serde_json::Value` directly so we avoid a cyclic dependency between
// `abp-integrations` and the `abp-*-sdk` crates.
// ---------------------------------------------------------------------------

fn build_user_content(wo: &WorkOrder) -> String {
    let mut content = wo.task.clone();
    for snippet in &wo.context.snippets {
        content.push_str(&format!("\n\n--- {} ---\n{}", snippet.name, snippet.content));
    }
    content
}

fn model_or_default<'a>(wo: &'a WorkOrder, fallback: &'a str) -> &'a str {
    wo.config.model.as_deref().unwrap_or(fallback)
}

fn wo_to_claude(wo: &WorkOrder) -> serde_json::Value {
    json!({
        "model": model_or_default(wo, "claude-sonnet-4-20250514"),
        "max_tokens": 4096,
        "system": null,
        "messages": [{
            "role": "user",
            "content": build_user_content(wo),
        }],
    })
}

fn wo_to_codex(wo: &WorkOrder) -> serde_json::Value {
    json!({
        "model": model_or_default(wo, "codex-mini-latest"),
        "input": [{
            "type": "message",
            "role": "user",
            "content": build_user_content(wo),
        }],
        "max_output_tokens": 4096,
    })
}

fn wo_to_gemini(wo: &WorkOrder) -> serde_json::Value {
    json!({
        "model": model_or_default(wo, "gemini-2.5-flash"),
        "contents": [{
            "role": "user",
            "parts": [{ "Text": build_user_content(wo) }],
        }],
        "generation_config": {
            "maxOutputTokens": 4096,
        },
    })
}

fn wo_to_kimi(wo: &WorkOrder) -> serde_json::Value {
    json!({
        "model": model_or_default(wo, "moonshot-v1-8k"),
        "messages": [{
            "role": "user",
            "content": build_user_content(wo),
        }],
        "max_tokens": 4096,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Translate a [`WorkOrder`] from one dialect to another.
///
/// Free-function form of [`ProjectionMatrix::translate`].
pub fn translate(from: Dialect, to: Dialect, wo: &WorkOrder) -> Result<serde_json::Value> {
    // Identity: same dialect in and out.
    if from == to {
        return Ok(serde_json::to_value(wo)?);
    }

    // ABP → vendor translations.
    if from == Dialect::Abp {
        return Ok(match to {
            Dialect::Claude => wo_to_claude(wo),
            Dialect::Codex => wo_to_codex(wo),
            Dialect::Gemini => wo_to_gemini(wo),
            Dialect::Kimi => wo_to_kimi(wo),
            Dialect::Abp => unreachable!("handled by identity branch"),
        });
    }

    bail!(
        "unsupported translation: {:?} -> {:?} (v0.1 supports identity and ABP-to-vendor only)",
        from,
        to
    )
}

/// List all translation pairs the matrix currently supports.
pub fn supported_translations() -> Vec<(Dialect, Dialect)> {
    let mut pairs = Vec::new();

    // Identity pairs.
    for &d in Dialect::ALL {
        pairs.push((d, d));
    }

    // ABP → each vendor dialect.
    for &d in Dialect::ALL {
        if d != Dialect::Abp {
            pairs.push((Dialect::Abp, d));
        }
    }

    pairs
}
