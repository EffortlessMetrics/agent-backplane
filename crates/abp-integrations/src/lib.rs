//! abp-integrations
//!
//! Compatibility facade over backend microcrates.

pub use abp_backend_core::{
    Backend, ensure_capability_requirements, extract_execution_mode,
    validate_passthrough_compatibility,
};
pub use abp_backend_mock::MockBackend;
pub use abp_backend_sidecar::SidecarBackend;
