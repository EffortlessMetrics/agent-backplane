// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Thread-safe capability registry for per-backend capability declarations.
//!
//! [`SharedCapabilityRegistry`] wraps an inner store behind `Arc<RwLock<>>`
//! so it can be shared across threads safely.

use crate::{
    EmulationStrategy, NegotiationResult, SupportLevel, check_capability, negotiate_capabilities,
};
use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

// ---------------------------------------------------------------------------
// CapabilitySet
// ---------------------------------------------------------------------------

/// A named set of capabilities with their support levels.
///
/// Wraps a [`CapabilityManifest`] with an optional description.
///
/// # Examples
///
/// ```
/// use abp_capability::registry::CapabilitySet;
/// use abp_core::{Capability, SupportLevel as CoreSupportLevel};
/// use std::collections::BTreeMap;
///
/// let mut manifest = BTreeMap::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
/// let set = CapabilitySet {
///     manifest,
///     description: Some("Test backend".into()),
/// };
/// assert!(set.supports(&Capability::Streaming));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySet {
    /// The underlying manifest mapping capabilities to support levels.
    pub manifest: CapabilityManifest,
    /// Optional human-readable description of this capability set.
    pub description: Option<String>,
}

impl CapabilitySet {
    /// Create a new capability set from a manifest.
    #[must_use]
    pub fn new(manifest: CapabilityManifest) -> Self {
        Self {
            manifest,
            description: None,
        }
    }

    /// Create a new capability set with a description.
    #[must_use]
    pub fn with_description(manifest: CapabilityManifest, description: impl Into<String>) -> Self {
        Self {
            manifest,
            description: Some(description.into()),
        }
    }

    /// Returns `true` if the capability is present and not `Unsupported`.
    #[must_use]
    pub fn supports(&self, cap: &Capability) -> bool {
        matches!(
            self.manifest.get(cap),
            Some(CoreSupportLevel::Native)
                | Some(CoreSupportLevel::Emulated)
                | Some(CoreSupportLevel::Restricted { .. })
        )
    }

    /// Get the support level for a capability.
    #[must_use]
    pub fn get(&self, cap: &Capability) -> Option<&CoreSupportLevel> {
        self.manifest.get(cap)
    }

    /// Number of capabilities in this set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.manifest.len()
    }

    /// Returns `true` if the set has no capabilities.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.manifest.is_empty()
    }
}

impl fmt::Display for CapabilitySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let native = self
            .manifest
            .values()
            .filter(|l| matches!(l, CoreSupportLevel::Native))
            .count();
        let emulated = self
            .manifest
            .values()
            .filter(|l| matches!(l, CoreSupportLevel::Emulated))
            .count();
        write!(
            f,
            "{} capabilities ({} native, {} emulated)",
            self.manifest.len(),
            native,
            emulated,
        )
    }
}

// ---------------------------------------------------------------------------
// BackendEntry
// ---------------------------------------------------------------------------

/// A registered backend with its capability set and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendEntry {
    /// Unique identifier for this backend.
    pub backend_id: String,
    /// The backend's capability set.
    pub capabilities: CapabilitySet,
}

// ---------------------------------------------------------------------------
// SharedCapabilityRegistry
// ---------------------------------------------------------------------------

/// Thread-safe capability registry storing per-backend capability declarations.
///
/// Uses `Arc<RwLock<>>` for interior mutability, allowing safe concurrent
/// reads and exclusive writes across threads.
///
/// # Examples
///
/// ```
/// use abp_capability::registry::{SharedCapabilityRegistry, CapabilitySet};
/// use abp_core::{Capability, SupportLevel as CoreSupportLevel};
/// use std::collections::BTreeMap;
///
/// let registry = SharedCapabilityRegistry::new();
///
/// let mut manifest = BTreeMap::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
/// registry.register("my-backend", CapabilitySet::new(manifest));
///
/// let caps = registry.lookup("my-backend");
/// assert!(caps.is_some());
/// ```
#[derive(Debug, Clone)]
pub struct SharedCapabilityRegistry {
    inner: Arc<RwLock<BTreeMap<String, CapabilitySet>>>,
}

impl Default for SharedCapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedCapabilityRegistry {
    /// Create a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Register a backend's capabilities.
    ///
    /// If a backend with the same ID already exists, it is replaced.
    pub fn register(&self, backend_id: &str, capabilities: CapabilitySet) {
        let mut store = self.inner.write().expect("registry lock poisoned");
        store.insert(backend_id.to_owned(), capabilities);
    }

    /// Remove a backend from the registry.
    ///
    /// Returns `true` if the backend existed and was removed.
    pub fn unregister(&self, backend_id: &str) -> bool {
        let mut store = self.inner.write().expect("registry lock poisoned");
        store.remove(backend_id).is_some()
    }

    /// Look up the capability set for a backend.
    ///
    /// Returns a cloned [`CapabilitySet`] if found.
    #[must_use]
    pub fn lookup(&self, backend_id: &str) -> Option<CapabilitySet> {
        let store = self.inner.read().expect("registry lock poisoned");
        store.get(backend_id).cloned()
    }

    /// Query which backends support a given capability and at what level.
    ///
    /// Returns a list of `(backend_id, SupportLevel)` pairs for every
    /// registered backend.
    #[must_use]
    pub fn query(&self, capability_name: &Capability) -> Vec<(String, SupportLevel)> {
        let store = self.inner.read().expect("registry lock poisoned");
        store
            .iter()
            .map(|(id, cap_set)| {
                let level = check_capability(&cap_set.manifest, capability_name);
                (id.clone(), level)
            })
            .collect()
    }

    /// Find all backends that satisfy every requirement in `requirements`.
    ///
    /// A backend satisfies a requirement if the capability is present and
    /// not `Unsupported` in its manifest.
    #[must_use]
    pub fn find_backends_supporting(&self, requirements: &[Capability]) -> Vec<String> {
        let store = self.inner.read().expect("registry lock poisoned");
        store
            .iter()
            .filter(|(_, cap_set)| requirements.iter().all(|cap| cap_set.supports(cap)))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Return all registered backend IDs.
    #[must_use]
    pub fn backend_ids(&self) -> Vec<String> {
        let store = self.inner.read().expect("registry lock poisoned");
        store.keys().cloned().collect()
    }

    /// Return the number of registered backends.
    #[must_use]
    pub fn len(&self) -> usize {
        let store = self.inner.read().expect("registry lock poisoned");
        store.len()
    }

    /// Returns `true` if no backends are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let store = self.inner.read().expect("registry lock poisoned");
        store.is_empty()
    }

    /// Negotiate required capabilities against a specific backend.
    ///
    /// Returns `None` if the backend is not registered.
    #[must_use]
    pub fn negotiate(
        &self,
        backend_id: &str,
        required: &[Capability],
    ) -> Option<NegotiationResult> {
        let store = self.inner.read().expect("registry lock poisoned");
        store
            .get(backend_id)
            .map(|cap_set| negotiate_capabilities(required, &cap_set.manifest))
    }

    /// Find the best backend for the given requirements.
    ///
    /// Among backends that have zero unsupported capabilities, selects the one
    /// with the most native support. Ties are broken by backend ID (alphabetical).
    #[must_use]
    pub fn best_backend_for(
        &self,
        requirements: &[Capability],
    ) -> Option<(String, NegotiationResult)> {
        let store = self.inner.read().expect("registry lock poisoned");
        store
            .iter()
            .map(|(id, cap_set)| {
                let result = negotiate_capabilities(requirements, &cap_set.manifest);
                (id.clone(), result)
            })
            .filter(|(_, result)| result.is_viable())
            .max_by(|(id_a, a), (id_b, b)| {
                a.native
                    .len()
                    .cmp(&b.native.len())
                    .then_with(|| id_b.cmp(id_a))
            })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{Capability, SupportLevel as CoreSupportLevel};
    use std::collections::BTreeMap;

    fn make_set(entries: &[(Capability, CoreSupportLevel)]) -> CapabilitySet {
        let manifest: CapabilityManifest = entries.iter().cloned().collect();
        CapabilitySet {
            manifest,
            description: None,
        }
    }

    fn make_set_with_desc(entries: &[(Capability, CoreSupportLevel)], desc: &str) -> CapabilitySet {
        let manifest: CapabilityManifest = entries.iter().cloned().collect();
        CapabilitySet {
            manifest,
            description: Some(desc.to_owned()),
        }
    }

    // ---- CapabilitySet ---------------------------------------------------

    #[test]
    fn capability_set_supports() {
        let set = make_set(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
            (Capability::Vision, CoreSupportLevel::Unsupported),
        ]);
        assert!(set.supports(&Capability::Streaming));
        assert!(set.supports(&Capability::ToolUse));
        assert!(!set.supports(&Capability::Vision));
        assert!(!set.supports(&Capability::Audio)); // not present
    }

    #[test]
    fn capability_set_restricted_counts_as_supported() {
        let set = make_set(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        )]);
        assert!(set.supports(&Capability::ToolBash));
    }

    #[test]
    fn capability_set_len_and_empty() {
        let empty = make_set(&[]);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let set = make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        assert!(!set.is_empty());
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn capability_set_display() {
        let set = make_set(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
            (Capability::Vision, CoreSupportLevel::Unsupported),
        ]);
        let s = format!("{set}");
        assert!(s.contains("3 capabilities"));
        assert!(s.contains("1 native"));
        assert!(s.contains("1 emulated"));
    }

    #[test]
    fn capability_set_with_description() {
        let set = CapabilitySet::with_description(BTreeMap::new(), "My backend");
        assert_eq!(set.description, Some("My backend".to_owned()));
    }

    #[test]
    fn capability_set_serde_roundtrip() {
        let set = make_set_with_desc(
            &[
                (Capability::Streaming, CoreSupportLevel::Native),
                (Capability::ToolUse, CoreSupportLevel::Emulated),
            ],
            "test",
        );
        let json = serde_json::to_string(&set).unwrap();
        let back: CapabilitySet = serde_json::from_str(&json).unwrap();
        assert_eq!(back.manifest.len(), 2);
        assert_eq!(back.description, Some("test".to_owned()));
    }

    // ---- SharedCapabilityRegistry: CRUD ----------------------------------

    #[test]
    fn registry_new_is_empty() {
        let reg = SharedCapabilityRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registry_register_and_lookup() {
        let reg = SharedCapabilityRegistry::new();
        let set = make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        reg.register("backend-a", set.clone());

        let found = reg.lookup("backend-a");
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.manifest.len(), 1);
        assert!(found.supports(&Capability::Streaming));
    }

    #[test]
    fn registry_lookup_missing() {
        let reg = SharedCapabilityRegistry::new();
        assert!(reg.lookup("nonexistent").is_none());
    }

    #[test]
    fn registry_register_overwrites() {
        let reg = SharedCapabilityRegistry::new();
        let set1 = make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let set2 = make_set(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        reg.register("backend-a", set1);
        assert_eq!(reg.lookup("backend-a").unwrap().manifest.len(), 1);

        reg.register("backend-a", set2);
        assert_eq!(reg.lookup("backend-a").unwrap().manifest.len(), 2);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn registry_unregister() {
        let reg = SharedCapabilityRegistry::new();
        let set = make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        reg.register("backend-a", set);
        assert!(!reg.is_empty());

        assert!(reg.unregister("backend-a"));
        assert!(reg.is_empty());
        assert!(reg.lookup("backend-a").is_none());
    }

    #[test]
    fn registry_unregister_missing_returns_false() {
        let reg = SharedCapabilityRegistry::new();
        assert!(!reg.unregister("nonexistent"));
    }

    #[test]
    fn registry_backend_ids() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "alpha",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        reg.register(
            "beta",
            make_set(&[(Capability::ToolUse, CoreSupportLevel::Native)]),
        );

        let ids = reg.backend_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"alpha".to_owned()));
        assert!(ids.contains(&"beta".to_owned()));
    }

    // ---- query -----------------------------------------------------------

    #[test]
    fn registry_query_capability() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "backend-a",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        reg.register(
            "backend-b",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Emulated)]),
        );
        reg.register("backend-c", make_set(&[]));

        let results = reg.query(&Capability::Streaming);
        assert_eq!(results.len(), 3);

        let a = results.iter().find(|(id, _)| id == "backend-a").unwrap();
        assert!(matches!(a.1, SupportLevel::Native));

        let b = results.iter().find(|(id, _)| id == "backend-b").unwrap();
        assert!(matches!(b.1, SupportLevel::Emulated { .. }));

        let c = results.iter().find(|(id, _)| id == "backend-c").unwrap();
        assert!(matches!(c.1, SupportLevel::Unsupported { .. }));
    }

    // ---- find_backends_supporting ----------------------------------------

    #[test]
    fn registry_find_backends_supporting_all() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "full",
            make_set(&[
                (Capability::Streaming, CoreSupportLevel::Native),
                (Capability::ToolUse, CoreSupportLevel::Native),
            ]),
        );
        reg.register(
            "partial",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );

        let found = reg.find_backends_supporting(&[Capability::Streaming, Capability::ToolUse]);
        assert_eq!(found, vec!["full".to_owned()]);
    }

    #[test]
    fn registry_find_backends_supporting_empty_requirements() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "backend-a",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        reg.register("backend-b", make_set(&[]));

        let found = reg.find_backends_supporting(&[]);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn registry_find_backends_supporting_none_match() {
        let reg = SharedCapabilityRegistry::new();
        reg.register("backend-a", make_set(&[]));

        let found = reg.find_backends_supporting(&[Capability::Streaming]);
        assert!(found.is_empty());
    }

    #[test]
    fn registry_find_backends_supporting_emulated_counts() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "emulated-backend",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Emulated)]),
        );

        let found = reg.find_backends_supporting(&[Capability::Streaming]);
        assert_eq!(found, vec!["emulated-backend".to_owned()]);
    }

    // ---- negotiate -------------------------------------------------------

    #[test]
    fn registry_negotiate_found() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "backend-a",
            make_set(&[
                (Capability::Streaming, CoreSupportLevel::Native),
                (Capability::ToolUse, CoreSupportLevel::Emulated),
            ]),
        );

        let result = reg
            .negotiate(
                "backend-a",
                &[
                    Capability::Streaming,
                    Capability::ToolUse,
                    Capability::Vision,
                ],
            )
            .unwrap();
        assert_eq!(result.native.len(), 1);
        assert_eq!(result.emulated.len(), 1);
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn registry_negotiate_missing_backend() {
        let reg = SharedCapabilityRegistry::new();
        assert!(
            reg.negotiate("nonexistent", &[Capability::Streaming])
                .is_none()
        );
    }

    // ---- best_backend_for ------------------------------------------------

    #[test]
    fn registry_best_backend_prefers_native() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "all-emulated",
            make_set(&[
                (Capability::Streaming, CoreSupportLevel::Emulated),
                (Capability::ToolUse, CoreSupportLevel::Emulated),
            ]),
        );
        reg.register(
            "all-native",
            make_set(&[
                (Capability::Streaming, CoreSupportLevel::Native),
                (Capability::ToolUse, CoreSupportLevel::Native),
            ]),
        );

        let (id, result) = reg
            .best_backend_for(&[Capability::Streaming, Capability::ToolUse])
            .unwrap();
        assert_eq!(id, "all-native");
        assert_eq!(result.native.len(), 2);
    }

    #[test]
    fn registry_best_backend_none_viable() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "backend-a",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );

        let result = reg.best_backend_for(&[Capability::Streaming, Capability::Vision]);
        assert!(result.is_none());
    }

    #[test]
    fn registry_best_backend_empty_requirements() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "backend-a",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );

        let result = reg.best_backend_for(&[]);
        assert!(result.is_some());
    }

    // ---- thread safety ---------------------------------------------------

    #[test]
    fn registry_clone_shares_state() {
        let reg1 = SharedCapabilityRegistry::new();
        let reg2 = reg1.clone();

        reg1.register(
            "backend-a",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        assert!(reg2.lookup("backend-a").is_some());
    }

    #[test]
    fn registry_thread_safe_concurrent_reads() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "backend-a",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let reg = reg.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        let result = reg.lookup("backend-a");
                        assert!(result.is_some());
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }
    }

    #[test]
    fn registry_thread_safe_concurrent_writes() {
        let reg = SharedCapabilityRegistry::new();

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let reg = reg.clone();
                std::thread::spawn(move || {
                    let id = format!("backend-{i}");
                    let set = make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]);
                    reg.register(&id, set);
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }

        assert_eq!(reg.len(), 8);
    }

    #[test]
    fn registry_thread_safe_mixed_read_write() {
        let reg = SharedCapabilityRegistry::new();
        reg.register(
            "initial",
            make_set(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let reg = reg.clone();
                std::thread::spawn(move || {
                    if i % 2 == 0 {
                        let id = format!("writer-{i}");
                        reg.register(
                            &id,
                            make_set(&[(Capability::ToolUse, CoreSupportLevel::Native)]),
                        );
                    } else {
                        // Readers always see "initial"
                        let _ = reg.lookup("initial");
                        let _ = reg.query(&Capability::Streaming);
                        let _ = reg.find_backends_supporting(&[Capability::Streaming]);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }
    }

    // ---- BackendEntry serde roundtrip ------------------------------------

    #[test]
    fn backend_entry_serde_roundtrip() {
        let entry = BackendEntry {
            backend_id: "test-backend".to_owned(),
            capabilities: make_set_with_desc(
                &[(Capability::Streaming, CoreSupportLevel::Native)],
                "A test backend",
            ),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: BackendEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backend_id, "test-backend");
        assert_eq!(
            back.capabilities.description,
            Some("A test backend".to_owned())
        );
    }

    // ---- default impl ---------------------------------------------------

    #[test]
    fn registry_default_is_empty() {
        let reg = SharedCapabilityRegistry::default();
        assert!(reg.is_empty());
    }
}
