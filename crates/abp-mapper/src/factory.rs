// SPDX-License-Identifier: MIT OR Apache-2.0

//! Default mapper factory for resolving IR mappers by dialect pair.

use abp_dialect::Dialect;

use crate::ir_claude_gemini::ClaudeGeminiIrMapper;
use crate::ir_claude_kimi::ClaudeKimiIrMapper;
use crate::ir_identity::IrIdentityMapper;
use crate::ir_mapper::IrMapper;
use crate::ir_openai_claude::OpenAiClaudeIrMapper;
use crate::ir_openai_codex::OpenAiCodexIrMapper;
use crate::ir_openai_gemini::OpenAiGeminiIrMapper;
use crate::ir_openai_kimi::OpenAiKimiIrMapper;

/// Returns the appropriate [`IrMapper`] implementation for a given
/// `(source, target)` dialect pair.
///
/// Returns `None` if no mapper is registered for the pair.
///
/// Supported pairs:
/// - Same-dialect → [`IrIdentityMapper`]
/// - OpenAI ↔ Claude → [`OpenAiClaudeIrMapper`]
/// - OpenAI ↔ Gemini → [`OpenAiGeminiIrMapper`]
/// - Claude ↔ Gemini → [`ClaudeGeminiIrMapper`]
/// - OpenAI ↔ Codex → [`OpenAiCodexIrMapper`] (lossy — Codex is output-only)
/// - OpenAI ↔ Kimi → [`OpenAiKimiIrMapper`]
/// - Claude ↔ Kimi → [`ClaudeKimiIrMapper`]
#[must_use]
pub fn default_ir_mapper(from: Dialect, to: Dialect) -> Option<Box<dyn IrMapper>> {
    if from == to {
        return Some(Box::new(IrIdentityMapper));
    }

    match (from, to) {
        (Dialect::OpenAi, Dialect::Claude) | (Dialect::Claude, Dialect::OpenAi) => {
            Some(Box::new(OpenAiClaudeIrMapper))
        }
        (Dialect::OpenAi, Dialect::Gemini) | (Dialect::Gemini, Dialect::OpenAi) => {
            Some(Box::new(OpenAiGeminiIrMapper))
        }
        (Dialect::Claude, Dialect::Gemini) | (Dialect::Gemini, Dialect::Claude) => {
            Some(Box::new(ClaudeGeminiIrMapper))
        }
        (Dialect::OpenAi, Dialect::Codex) | (Dialect::Codex, Dialect::OpenAi) => {
            Some(Box::new(OpenAiCodexIrMapper))
        }
        (Dialect::OpenAi, Dialect::Kimi) | (Dialect::Kimi, Dialect::OpenAi) => {
            Some(Box::new(OpenAiKimiIrMapper))
        }
        (Dialect::Claude, Dialect::Kimi) | (Dialect::Kimi, Dialect::Claude) => {
            Some(Box::new(ClaudeKimiIrMapper))
        }
        _ => None,
    }
}

/// Returns all dialect pairs for which a default IR mapper is available.
#[must_use]
pub fn supported_ir_pairs() -> Vec<(Dialect, Dialect)> {
    let mut pairs: Vec<(Dialect, Dialect)> = Vec::new();

    // Identity pairs
    for &d in Dialect::all() {
        pairs.push((d, d));
    }

    // Cross-dialect pairs
    pairs.push((Dialect::OpenAi, Dialect::Claude));
    pairs.push((Dialect::Claude, Dialect::OpenAi));
    pairs.push((Dialect::OpenAi, Dialect::Gemini));
    pairs.push((Dialect::Gemini, Dialect::OpenAi));
    pairs.push((Dialect::Claude, Dialect::Gemini));
    pairs.push((Dialect::Gemini, Dialect::Claude));
    pairs.push((Dialect::OpenAi, Dialect::Codex));
    pairs.push((Dialect::Codex, Dialect::OpenAi));
    pairs.push((Dialect::OpenAi, Dialect::Kimi));
    pairs.push((Dialect::Kimi, Dialect::OpenAi));
    pairs.push((Dialect::Claude, Dialect::Kimi));
    pairs.push((Dialect::Kimi, Dialect::Claude));

    pairs
}
