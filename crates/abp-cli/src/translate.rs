// SPDX-License-Identifier: MIT OR Apache-2.0
//! Translate subcommand implementation.
//!
//! Translates JSON requests between agent SDK dialect formats using the
//! IR-level mapper from `abp-mapper`.

use abp_core::ir::IrConversation;
use abp_dialect::Dialect;
use abp_mapper::{default_ir_mapper, supported_ir_pairs};
use anyhow::{Context, Result};
use std::path::Path;

/// Parse a dialect name string into a [`Dialect`] enum value.
pub fn parse_dialect(name: &str) -> Result<Dialect> {
    match name.to_ascii_lowercase().as_str() {
        "openai" | "open_ai" => Ok(Dialect::OpenAi),
        "claude" => Ok(Dialect::Claude),
        "gemini" => Ok(Dialect::Gemini),
        "codex" => Ok(Dialect::Codex),
        "kimi" => Ok(Dialect::Kimi),
        "copilot" => Ok(Dialect::Copilot),
        _ => anyhow::bail!(
            "unknown dialect '{}'; expected one of: openai, claude, gemini, codex, kimi, copilot",
            name
        ),
    }
}

/// List all supported translation pairs as `(source, target)`.
pub fn list_supported_pairs() -> Vec<(Dialect, Dialect)> {
    supported_ir_pairs()
}

/// Translate a JSON request from one dialect to another.
///
/// Returns the translated JSON value on success.
pub fn translate_request(
    from: Dialect,
    to: Dialect,
    input: &serde_json::Value,
) -> Result<serde_json::Value> {
    let mapper = default_ir_mapper(from, to)
        .with_context(|| format!("no mapper available for {} -> {}", from.label(), to.label()))?;

    let ir: IrConversation =
        serde_json::from_value(input.clone()).context("parse input as IR conversation")?;

    let mapped = mapper
        .map_request(from, to, &ir)
        .map_err(|e| anyhow::anyhow!("translation failed: {e}"))?;

    serde_json::to_value(&mapped).context("serialize mapped conversation")
}

/// Translate a JSON file from one dialect to another, returning the result as
/// a pretty-printed JSON string.
pub fn translate_file(from: Dialect, to: Dialect, path: &Path) -> Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("read input file '{}'", path.display()))?;

    let value: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parse JSON from '{}'", path.display()))?;

    let result = translate_request(from, to, &value)?;
    serde_json::to_string_pretty(&result).context("serialize translated output")
}

/// Translate a JSON string from one dialect to another.
pub fn translate_json_str(from: Dialect, to: Dialect, json_str: &str) -> Result<String> {
    let value: serde_json::Value =
        serde_json::from_str(json_str).context("parse input JSON string")?;
    let result = translate_request(from, to, &value)?;
    serde_json::to_string_pretty(&result).context("serialize translated output")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_dialect_openai() {
        assert_eq!(parse_dialect("openai").unwrap(), Dialect::OpenAi);
        assert_eq!(parse_dialect("OpenAI").unwrap(), Dialect::OpenAi);
        assert_eq!(parse_dialect("open_ai").unwrap(), Dialect::OpenAi);
    }

    #[test]
    fn parse_dialect_claude() {
        assert_eq!(parse_dialect("claude").unwrap(), Dialect::Claude);
        assert_eq!(parse_dialect("CLAUDE").unwrap(), Dialect::Claude);
    }

    #[test]
    fn parse_dialect_gemini() {
        assert_eq!(parse_dialect("gemini").unwrap(), Dialect::Gemini);
    }

    #[test]
    fn parse_dialect_codex() {
        assert_eq!(parse_dialect("codex").unwrap(), Dialect::Codex);
    }

    #[test]
    fn parse_dialect_kimi() {
        assert_eq!(parse_dialect("kimi").unwrap(), Dialect::Kimi);
    }

    #[test]
    fn parse_dialect_copilot() {
        assert_eq!(parse_dialect("copilot").unwrap(), Dialect::Copilot);
    }

    #[test]
    fn parse_dialect_unknown_errors() {
        assert!(parse_dialect("foobar").is_err());
    }

    #[test]
    fn list_pairs_is_nonempty() {
        let pairs = list_supported_pairs();
        assert!(!pairs.is_empty());
    }

    #[test]
    fn identity_translation_returns_json() {
        let input = json!({"model": "gpt-4", "messages": []});
        let result = translate_request(Dialect::OpenAi, Dialect::OpenAi, &input).unwrap();
        assert!(
            result.is_object(),
            "translation should return a JSON object"
        );
    }

    #[test]
    fn translate_json_str_returns_valid_json() {
        let input = r#"{"model": "gpt-4", "messages": []}"#;
        let result = translate_json_str(Dialect::OpenAi, Dialect::OpenAi, input).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn translate_file_reads_and_translates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.json");
        std::fs::write(&path, r#"{"model": "gpt-4", "messages": []}"#).unwrap();
        let result = translate_file(Dialect::OpenAi, Dialect::OpenAi, &path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn translate_file_missing_file_errors() {
        assert!(translate_file(
            Dialect::OpenAi,
            Dialect::OpenAi,
            std::path::Path::new("/nonexistent.json")
        )
        .is_err());
    }

    #[test]
    fn translate_file_invalid_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(translate_file(Dialect::OpenAi, Dialect::OpenAi, &path).is_err());
    }
}
