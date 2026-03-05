// SPDX-License-Identifier: MIT OR Apache-2.0

//! Identity (passthrough) IR mapper — returns conversations unchanged.

use abp_core::ir::IrConversation;
use abp_dialect::Dialect;

use crate::ir_mapper::IrMapper;
use crate::MapError;

/// A no-op IR mapper that returns conversations unchanged.
///
/// Useful for same-dialect routing and as a baseline for testing.
pub struct IrIdentityMapper;

impl IrMapper for IrIdentityMapper {
    fn map_request(
        &self,
        _from: Dialect,
        _to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        Ok(ir.clone())
    }

    fn map_response(
        &self,
        _from: Dialect,
        _to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        Ok(ir.clone())
    }

    fn supported_pairs(&self) -> Vec<(Dialect, Dialect)> {
        Dialect::all().iter().map(|&d| (d, d)).collect()
    }
}
