//! abp-integrations
//!
//! Backwards-compatible facade for integration-related crates.

pub use abp_backend::{
    Backend, ensure_capability_requirements, extract_execution_mode,
    validate_passthrough_compatibility,
};
pub use abp_backend_mock::MockBackend;
pub use abp_backend_sidecar::SidecarBackend;
