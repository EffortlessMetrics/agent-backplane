// SPDX-License-Identifier: MIT OR Apache-2.0

//! The [`IrMapper`] trait for IR-level cross-dialect translation.

use abp_core::ir::IrConversation;
use abp_dialect::Dialect;

use crate::MapError;

/// Core trait for translating conversations between agent-SDK dialects at the
/// IR level.
///
/// Implementations are pure data transformations with no I/O. Each mapper
/// covers one or more directional dialect pairs.
pub trait IrMapper: Send + Sync {
    /// Translate an IR conversation representing a *request* from one dialect
    /// to another.
    fn map_request(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError>;

    /// Translate an IR conversation representing a *response* from one dialect
    /// to another.
    fn map_response(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError>;

    /// Returns the set of `(source, target)` dialect pairs this mapper supports.
    fn supported_pairs(&self) -> Vec<(Dialect, Dialect)>;
}
