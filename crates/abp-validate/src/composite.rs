// SPDX-License-Identifier: MIT OR Apache-2.0
//! Composite validator that chains multiple validators and aggregates results.

use crate::{ValidationErrors, Validator};

/// Type alias for boxed validator closures.
type ValidatorFn<T> = Box<dyn Fn(&T) -> Result<(), ValidationErrors> + Send + Sync>;

/// Chains multiple [`Validator<T>`] implementations and aggregates their errors.
///
/// All validators are run regardless of earlier failures, so the caller
/// receives a complete picture of all validation issues.
pub struct CompositeValidator<T> {
    validators: Vec<ValidatorFn<T>>,
}

impl<T> std::fmt::Debug for CompositeValidator<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeValidator")
            .field("count", &self.validators.len())
            .finish()
    }
}

impl<T> Default for CompositeValidator<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> CompositeValidator<T> {
    /// Create an empty composite validator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            validators: Vec::new(),
        }
    }

    /// Add a validator to the chain.
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn add<V>(mut self, v: V) -> Self
    where
        V: Validator<T> + Send + Sync + 'static,
    {
        self.validators
            .push(Box::new(move |value| v.validate(value)));
        self
    }

    /// Number of validators in the chain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.validators.len()
    }

    /// Whether the chain is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.validators.is_empty()
    }
}

impl<T> Validator<T> for CompositeValidator<T> {
    fn validate(&self, value: &T) -> Result<(), ValidationErrors> {
        let mut all_errs = ValidationErrors::new();

        for v in &self.validators {
            if let Err(errs) = v(value) {
                all_errs.merge(errs);
            }
        }

        all_errs.into_result()
    }
}
