// SPDX-License-Identifier: MIT OR Apache-2.0
//! Copilot references and confirmations.
//!
//! This module provides convenience constructors and re-exports for the
//! Copilot-specific reference and confirmation types used in the Extensions API.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// Re-export the canonical SDK types.
pub use abp_copilot_sdk::dialect::{CopilotConfirmation, CopilotReference, CopilotReferenceType};

// ── Reference builders ──────────────────────────────────────────────────

/// Build a file reference.
#[must_use]
pub fn file_reference(id: impl Into<String>, path: impl Into<String>) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: id.into(),
        data: serde_json::json!({ "path": path.into() }),
        metadata: None,
    }
}

/// Build a snippet reference.
#[must_use]
pub fn snippet_reference(
    id: impl Into<String>,
    name: impl Into<String>,
    content: impl Into<String>,
) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::Snippet,
        id: id.into(),
        data: serde_json::json!({ "name": name.into(), "content": content.into() }),
        metadata: None,
    }
}

/// Build a repository reference.
#[must_use]
pub fn repository_reference(
    id: impl Into<String>,
    owner: impl Into<String>,
    name: impl Into<String>,
) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::Repository,
        id: id.into(),
        data: serde_json::json!({ "owner": owner.into(), "name": name.into() }),
        metadata: None,
    }
}

/// Build a web search result reference.
#[must_use]
pub fn web_search_reference(
    id: impl Into<String>,
    url: impl Into<String>,
    title: impl Into<String>,
) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::WebSearchResult,
        id: id.into(),
        data: serde_json::json!({ "url": url.into(), "title": title.into() }),
        metadata: None,
    }
}

/// Attach metadata to a reference.
#[must_use]
pub fn with_metadata(
    mut reference: CopilotReference,
    metadata: BTreeMap<String, serde_json::Value>,
) -> CopilotReference {
    reference.metadata = Some(metadata);
    reference
}

// ── Confirmation builders ───────────────────────────────────────────────

/// Build a pending confirmation (not yet accepted/rejected).
#[must_use]
pub fn pending_confirmation(
    id: impl Into<String>,
    title: impl Into<String>,
    message: impl Into<String>,
) -> CopilotConfirmation {
    CopilotConfirmation {
        id: id.into(),
        title: title.into(),
        message: message.into(),
        accepted: None,
    }
}

/// Build an accepted confirmation.
#[must_use]
pub fn accepted_confirmation(
    id: impl Into<String>,
    title: impl Into<String>,
    message: impl Into<String>,
) -> CopilotConfirmation {
    CopilotConfirmation {
        id: id.into(),
        title: title.into(),
        message: message.into(),
        accepted: Some(true),
    }
}

/// Build a rejected confirmation.
#[must_use]
pub fn rejected_confirmation(
    id: impl Into<String>,
    title: impl Into<String>,
    message: impl Into<String>,
) -> CopilotConfirmation {
    CopilotConfirmation {
        id: id.into(),
        title: title.into(),
        message: message.into(),
        accepted: Some(false),
    }
}

// ── Confirmation state ──────────────────────────────────────────────────

/// State of a confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationState {
    /// Waiting for user response.
    Pending,
    /// User accepted.
    Accepted,
    /// User rejected.
    Rejected,
}

/// Determine the state of a confirmation.
#[must_use]
pub fn confirmation_state(confirmation: &CopilotConfirmation) -> ConfirmationState {
    match confirmation.accepted {
        None => ConfirmationState::Pending,
        Some(true) => ConfirmationState::Accepted,
        Some(false) => ConfirmationState::Rejected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_reference_construction() {
        let r = file_reference("f-1", "src/main.rs");
        assert_eq!(r.ref_type, CopilotReferenceType::File);
        assert_eq!(r.id, "f-1");
        assert_eq!(r.data["path"], "src/main.rs");
        assert!(r.metadata.is_none());
    }

    #[test]
    fn snippet_reference_construction() {
        let r = snippet_reference("s-1", "helper.rs", "fn foo() {}");
        assert_eq!(r.ref_type, CopilotReferenceType::Snippet);
        assert_eq!(r.data["name"], "helper.rs");
        assert_eq!(r.data["content"], "fn foo() {}");
    }

    #[test]
    fn repository_reference_construction() {
        let r = repository_reference("r-1", "octocat", "hello-world");
        assert_eq!(r.ref_type, CopilotReferenceType::Repository);
        assert_eq!(r.data["owner"], "octocat");
        assert_eq!(r.data["name"], "hello-world");
    }

    #[test]
    fn web_search_reference_construction() {
        let r = web_search_reference("w-1", "https://example.com", "Example");
        assert_eq!(r.ref_type, CopilotReferenceType::WebSearchResult);
        assert_eq!(r.data["url"], "https://example.com");
        assert_eq!(r.data["title"], "Example");
    }

    #[test]
    fn with_metadata_attaches() {
        let r = file_reference("f-2", "lib.rs");
        let mut meta = BTreeMap::new();
        meta.insert("label".into(), serde_json::json!("Library"));
        let r = with_metadata(r, meta);
        assert!(r.metadata.is_some());
        assert_eq!(r.metadata.unwrap()["label"], "Library");
    }

    #[test]
    fn file_reference_serde_roundtrip() {
        let r = file_reference("f-3", "test.rs");
        let json = serde_json::to_string(&r).unwrap();
        let back: CopilotReference = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "f-3");
        assert_eq!(back.ref_type, CopilotReferenceType::File);
    }

    #[test]
    fn pending_confirmation_construction() {
        let c = pending_confirmation("c-1", "Delete file?", "This will delete main.rs");
        assert_eq!(c.id, "c-1");
        assert_eq!(c.title, "Delete file?");
        assert_eq!(c.message, "This will delete main.rs");
        assert!(c.accepted.is_none());
    }

    #[test]
    fn accepted_confirmation_construction() {
        let c = accepted_confirmation("c-2", "Approve", "Approved!");
        assert_eq!(c.accepted, Some(true));
    }

    #[test]
    fn rejected_confirmation_construction() {
        let c = rejected_confirmation("c-3", "Reject", "Rejected!");
        assert_eq!(c.accepted, Some(false));
    }

    #[test]
    fn confirmation_serde_roundtrip() {
        let c = pending_confirmation("c-4", "Confirm?", "Are you sure?");
        let json = serde_json::to_string(&c).unwrap();
        let back: CopilotConfirmation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "c-4");
        assert!(back.accepted.is_none());
    }

    #[test]
    fn confirmation_state_pending() {
        let c = pending_confirmation("c-5", "T", "M");
        assert_eq!(confirmation_state(&c), ConfirmationState::Pending);
    }

    #[test]
    fn confirmation_state_accepted() {
        let c = accepted_confirmation("c-6", "T", "M");
        assert_eq!(confirmation_state(&c), ConfirmationState::Accepted);
    }

    #[test]
    fn confirmation_state_rejected() {
        let c = rejected_confirmation("c-7", "T", "M");
        assert_eq!(confirmation_state(&c), ConfirmationState::Rejected);
    }

    #[test]
    fn confirmation_state_serde_roundtrip() {
        for state in [
            ConfirmationState::Pending,
            ConfirmationState::Accepted,
            ConfirmationState::Rejected,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let back: ConfirmationState = serde_json::from_str(&json).unwrap();
            assert_eq!(back, state);
        }
    }
}
