// SPDX-License-Identifier: MIT OR Apache-2.0
//! Validation for Agent Backplane work orders, receipts, events, and protocol envelopes.
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod composite;
/// Config validation against schema rules.
pub mod config;
mod dialect;
mod envelope;
mod error;
mod event;
mod receipt;
/// Structured validation report with severity levels.
pub mod report;
/// `ValidationRule` trait and built-in rule implementations.
pub mod rule;
mod rule_builder;
mod schema;
mod work_order;

pub use composite::CompositeValidator;
pub use config::ConfigValidator;
pub use dialect::{DialectRequestValidator, DialectResponseValidator};
pub use envelope::{EnvelopeValidator, RawEnvelopeValidator, validate_hello_version};
pub use error::{ValidationError, ValidationErrorKind, ValidationErrors};
pub use event::EventValidator;
pub use receipt::ReceiptValidator;
pub use report::ValidationReport;
pub use rule::ValidationRule;
pub use rule_builder::{CustomValidator, RuleBuilder};
pub use schema::{JsonType, SchemaValidator};
pub use work_order::WorkOrderValidator;

/// Trait for validating a value of type `T`.
pub trait Validator<T> {
    /// Validate the given value, returning accumulated errors on failure.
    fn validate(&self, value: &T) -> Result<(), ValidationErrors>;
}
