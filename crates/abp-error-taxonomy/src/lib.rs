#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![deny(unsafe_code)]
//! Re-exports from `abp-error` for taxonomy tests.

pub use abp_error::*;

pub mod classification;
pub use classification::{
    ClassificationCategory, ErrorClassification, ErrorClassifier, ErrorSeverity, RecoveryAction,
    RecoverySuggestion,
};
