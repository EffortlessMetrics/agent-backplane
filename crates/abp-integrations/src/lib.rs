//! Backward-compatible integration facade.
//!
//! Prefer depending directly on focused crates:
//! - `abp-backend`
//! - `abp-mock-backend`
//! - `abp-sidecar-backend`

pub use abp_backend::{Backend, ensure_capability_requirements};
pub use abp_mock_backend::MockBackend;
pub use abp_sidecar_backend::SidecarBackend;
