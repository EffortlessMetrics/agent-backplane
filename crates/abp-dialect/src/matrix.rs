// SPDX-License-Identifier: MIT OR Apache-2.0
//! Static feature matrix mapping every `(Dialect, DialectFeature)` to a
//! [`FeatureSupport`](crate::features::FeatureSupport) level.
//!
//! Use `feature_matrix` to get the full table, or `dialect_features`
//! to get the [`DialectFeatureSet`](crate::features::DialectFeatureSet) for
//! a single dialect.

use crate::Dialect;
use crate::features::{DialectFeature, DialectFeatureSet, FeatureSupport};

use DialectFeature::*;
use FeatureSupport::*;

/// Full feature matrix: `(Dialect, DialectFeature, FeatureSupport)`.
///
/// This is the authoritative table of known support levels across all
/// dialects and features.
#[rustfmt::skip]
pub static FEATURE_MATRIX: &[(Dialect, DialectFeature, FeatureSupport)] = &[
    // ── OpenAI ──────────────────────────────────────────────────────
    (Dialect::OpenAi, SystemMessages,    Native),
    (Dialect::OpenAi, ToolUse,           Native),
    (Dialect::OpenAi, Streaming,         Native),
    (Dialect::OpenAi, Vision,            Native),
    (Dialect::OpenAi, Audio,             Native),
    (Dialect::OpenAi, ExtendedThinking,  None),
    (Dialect::OpenAi, Caching,           None),
    (Dialect::OpenAi, ParallelToolCalls, Native),
    (Dialect::OpenAi, FunctionCalling,   Native),
    (Dialect::OpenAi, Embeddings,        Native),
    (Dialect::OpenAi, CodeExecution,     None),
    (Dialect::OpenAi, StructuredOutput,  Native),
    (Dialect::OpenAi, FileAttachments,   Native),
    (Dialect::OpenAi, WebSearch,         Emulated),
    (Dialect::OpenAi, MultimodalOutput,  Native),

    // ── Claude ──────────────────────────────────────────────────────
    (Dialect::Claude, SystemMessages,    Native),
    (Dialect::Claude, ToolUse,           Native),
    (Dialect::Claude, Streaming,         Native),
    (Dialect::Claude, Vision,            Native),
    (Dialect::Claude, Audio,             None),
    (Dialect::Claude, ExtendedThinking,  Native),
    (Dialect::Claude, Caching,           Native),
    (Dialect::Claude, ParallelToolCalls, Native),
    (Dialect::Claude, FunctionCalling,   Emulated),
    (Dialect::Claude, Embeddings,        None),
    (Dialect::Claude, CodeExecution,     Native),
    (Dialect::Claude, StructuredOutput,  Native),
    (Dialect::Claude, FileAttachments,   Native),
    (Dialect::Claude, WebSearch,         Native),
    (Dialect::Claude, MultimodalOutput,  None),

    // ── Gemini ──────────────────────────────────────────────────────
    (Dialect::Gemini, SystemMessages,    Native),
    (Dialect::Gemini, ToolUse,           Native),
    (Dialect::Gemini, Streaming,         Native),
    (Dialect::Gemini, Vision,            Native),
    (Dialect::Gemini, Audio,             Native),
    (Dialect::Gemini, ExtendedThinking,  Native),
    (Dialect::Gemini, Caching,           Native),
    (Dialect::Gemini, ParallelToolCalls, Native),
    (Dialect::Gemini, FunctionCalling,   Native),
    (Dialect::Gemini, Embeddings,        Native),
    (Dialect::Gemini, CodeExecution,     Native),
    (Dialect::Gemini, StructuredOutput,  Native),
    (Dialect::Gemini, FileAttachments,   Native),
    (Dialect::Gemini, WebSearch,         Native),
    (Dialect::Gemini, MultimodalOutput,  Native),

    // ── Codex ───────────────────────────────────────────────────────
    (Dialect::Codex, SystemMessages,    Native),
    (Dialect::Codex, ToolUse,           Native),
    (Dialect::Codex, Streaming,         Native),
    (Dialect::Codex, Vision,            Emulated),
    (Dialect::Codex, Audio,             None),
    (Dialect::Codex, ExtendedThinking,  Native),
    (Dialect::Codex, Caching,           None),
    (Dialect::Codex, ParallelToolCalls, Native),
    (Dialect::Codex, FunctionCalling,   Emulated),
    (Dialect::Codex, Embeddings,        None),
    (Dialect::Codex, CodeExecution,     Native),
    (Dialect::Codex, StructuredOutput,  Native),
    (Dialect::Codex, FileAttachments,   Native),
    (Dialect::Codex, WebSearch,         None),
    (Dialect::Codex, MultimodalOutput,  None),

    // ── Kimi ────────────────────────────────────────────────────────
    (Dialect::Kimi, SystemMessages,    Native),
    (Dialect::Kimi, ToolUse,           Native),
    (Dialect::Kimi, Streaming,         Native),
    (Dialect::Kimi, Vision,            Native),
    (Dialect::Kimi, Audio,             None),
    (Dialect::Kimi, ExtendedThinking,  None),
    (Dialect::Kimi, Caching,           None),
    (Dialect::Kimi, ParallelToolCalls, Emulated),
    (Dialect::Kimi, FunctionCalling,   Native),
    (Dialect::Kimi, Embeddings,        None),
    (Dialect::Kimi, CodeExecution,     None),
    (Dialect::Kimi, StructuredOutput,  Emulated),
    (Dialect::Kimi, FileAttachments,   Native),
    (Dialect::Kimi, WebSearch,         Native),
    (Dialect::Kimi, MultimodalOutput,  None),

    // ── Copilot ─────────────────────────────────────────────────────
    (Dialect::Copilot, SystemMessages,    Native),
    (Dialect::Copilot, ToolUse,           Native),
    (Dialect::Copilot, Streaming,         Native),
    (Dialect::Copilot, Vision,            Emulated),
    (Dialect::Copilot, Audio,             None),
    (Dialect::Copilot, ExtendedThinking,  None),
    (Dialect::Copilot, Caching,           None),
    (Dialect::Copilot, ParallelToolCalls, Native),
    (Dialect::Copilot, FunctionCalling,   Native),
    (Dialect::Copilot, Embeddings,        Emulated),
    (Dialect::Copilot, CodeExecution,     Emulated),
    (Dialect::Copilot, StructuredOutput,  Native),
    (Dialect::Copilot, FileAttachments,   Native),
    (Dialect::Copilot, WebSearch,         Emulated),
    (Dialect::Copilot, MultimodalOutput,  None),
];

/// Build a [`DialectFeatureSet`] for the given dialect from the static
/// [`FEATURE_MATRIX`].
#[must_use]
pub fn dialect_features(dialect: Dialect) -> DialectFeatureSet {
    DialectFeatureSet::from_iter(
        FEATURE_MATRIX
            .iter()
            .filter(|(d, _, _)| *d == dialect)
            .map(|(_, f, s)| (*f, *s)),
    )
}

/// Look up the support level for a specific `(dialect, feature)` pair.
///
/// Returns [`FeatureSupport::None`] when the pair is not in the matrix.
#[must_use]
pub fn feature_support(dialect: Dialect, feature: DialectFeature) -> FeatureSupport {
    FEATURE_MATRIX
        .iter()
        .find(|(d, f, _)| *d == dialect && *f == feature)
        .map(|(_, _, s)| *s)
        .unwrap_or(FeatureSupport::None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_covers_all_dialects() {
        for d in Dialect::all() {
            let count = FEATURE_MATRIX.iter().filter(|(dd, _, _)| dd == d).count();
            assert!(count > 0, "no matrix entries for {d:?}");
        }
    }

    #[test]
    fn matrix_covers_all_features_per_dialect() {
        for d in Dialect::all() {
            for f in DialectFeature::all() {
                let found = FEATURE_MATRIX.iter().any(|(dd, ff, _)| dd == d && ff == f);
                assert!(found, "missing matrix entry for ({d:?}, {f:?})");
            }
        }
    }

    #[test]
    fn matrix_total_entries() {
        // 6 dialects × 15 features = 90 entries
        assert_eq!(FEATURE_MATRIX.len(), 90);
    }

    #[test]
    fn dialect_features_openai() {
        let fs = dialect_features(Dialect::OpenAi);
        assert_eq!(fs.supports(DialectFeature::ToolUse), FeatureSupport::Native);
        assert_eq!(
            fs.supports(DialectFeature::ExtendedThinking),
            FeatureSupport::None
        );
        assert_eq!(
            fs.supports(DialectFeature::WebSearch),
            FeatureSupport::Emulated
        );
    }

    #[test]
    fn dialect_features_claude() {
        let fs = dialect_features(Dialect::Claude);
        assert_eq!(
            fs.supports(DialectFeature::ExtendedThinking),
            FeatureSupport::Native
        );
        assert_eq!(fs.supports(DialectFeature::Caching), FeatureSupport::Native);
        assert_eq!(fs.supports(DialectFeature::Audio), FeatureSupport::None);
        assert_eq!(
            fs.supports(DialectFeature::FunctionCalling),
            FeatureSupport::Emulated
        );
    }

    #[test]
    fn dialect_features_gemini() {
        let fs = dialect_features(Dialect::Gemini);
        // Gemini supports basically everything natively
        for f in DialectFeature::all() {
            assert_eq!(
                fs.supports(*f),
                FeatureSupport::Native,
                "expected native for Gemini/{f:?}"
            );
        }
    }

    #[test]
    fn dialect_features_codex() {
        let fs = dialect_features(Dialect::Codex);
        assert_eq!(
            fs.supports(DialectFeature::CodeExecution),
            FeatureSupport::Native
        );
        assert_eq!(
            fs.supports(DialectFeature::Vision),
            FeatureSupport::Emulated
        );
        assert_eq!(fs.supports(DialectFeature::Audio), FeatureSupport::None);
    }

    #[test]
    fn dialect_features_kimi() {
        let fs = dialect_features(Dialect::Kimi);
        assert_eq!(
            fs.supports(DialectFeature::WebSearch),
            FeatureSupport::Native
        );
        assert_eq!(
            fs.supports(DialectFeature::ParallelToolCalls),
            FeatureSupport::Emulated
        );
        assert_eq!(
            fs.supports(DialectFeature::Embeddings),
            FeatureSupport::None
        );
    }

    #[test]
    fn dialect_features_copilot() {
        let fs = dialect_features(Dialect::Copilot);
        assert_eq!(fs.supports(DialectFeature::ToolUse), FeatureSupport::Native);
        assert_eq!(
            fs.supports(DialectFeature::Vision),
            FeatureSupport::Emulated
        );
        assert_eq!(fs.supports(DialectFeature::Audio), FeatureSupport::None);
        assert_eq!(
            fs.supports(DialectFeature::CodeExecution),
            FeatureSupport::Emulated
        );
    }

    #[test]
    fn feature_support_lookup() {
        assert_eq!(
            feature_support(Dialect::OpenAi, DialectFeature::Streaming),
            FeatureSupport::Native
        );
        assert_eq!(
            feature_support(Dialect::Claude, DialectFeature::Audio),
            FeatureSupport::None
        );
    }

    #[test]
    fn dialect_features_set_len() {
        for d in Dialect::all() {
            let fs = dialect_features(*d);
            assert_eq!(fs.len(), DialectFeature::all().len());
        }
    }

    #[test]
    fn openai_native_features_include_tool_use_and_streaming() {
        let fs = dialect_features(Dialect::OpenAi);
        let native = fs.native_features();
        assert!(native.contains(&DialectFeature::ToolUse));
        assert!(native.contains(&DialectFeature::Streaming));
    }

    #[test]
    fn claude_emulated_features_include_function_calling() {
        let fs = dialect_features(Dialect::Claude);
        let emulated = fs.emulated_features();
        assert!(emulated.contains(&DialectFeature::FunctionCalling));
    }
}
