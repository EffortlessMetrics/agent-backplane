// SPDX-License-Identifier: MIT OR Apache-2.0
//! Runtime configuration loading and integration with `abp-config`.
//!
//! Provides [`RuntimeConfig`] — a runtime-specific view of configuration
//! covering backend selection strategy, default policies, telemetry settings,
//! and workspace options.  This module does **not** depend on `abp-config`
//! directly (to avoid a circular dependency); instead it defines its own
//! configuration types that can be populated from a parsed `BackplaneConfig`
//! or constructed programmatically.

use abp_core::PolicyProfile;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Backend selection strategy
// ---------------------------------------------------------------------------

/// Strategy used to select a backend when none is specified explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendSelectionStrategy {
    /// Always use a single named backend.
    Fixed {
        /// Name of the backend to use.
        name: String,
    },
    /// Use the projection matrix to pick the best-fit backend.
    Projection,
    /// Try backends in the given order; use the first that satisfies
    /// capability requirements.
    Fallback {
        /// Ordered list of backend names.
        chain: Vec<String>,
    },
}

impl Default for BackendSelectionStrategy {
    fn default() -> Self {
        Self::Fixed {
            name: "mock".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Telemetry settings
// ---------------------------------------------------------------------------

/// Telemetry and observability settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetrySettings {
    /// Whether run-level metrics collection is enabled.
    pub metrics_enabled: bool,
    /// Whether distributed-style trace spans are enabled.
    pub tracing_enabled: bool,
    /// How often (in runs) a metrics snapshot is logged.
    /// `None` disables periodic logging.
    pub log_interval_runs: Option<u64>,
}

impl Default for TelemetrySettings {
    fn default() -> Self {
        Self {
            metrics_enabled: true,
            tracing_enabled: true,
            log_interval_runs: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Workspace options
// ---------------------------------------------------------------------------

/// Options governing workspace preparation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceOptions {
    /// Base directory for staged workspaces.
    /// When `None`, the system temp directory is used.
    pub base_dir: Option<PathBuf>,
    /// Whether to auto-initialise a git repository in staged workspaces.
    pub git_init: bool,
    /// Whether to create an initial "baseline" commit.
    pub baseline_commit: bool,
}

impl Default for WorkspaceOptions {
    fn default() -> Self {
        Self {
            base_dir: None,
            git_init: true,
            baseline_commit: true,
        }
    }
}

// ---------------------------------------------------------------------------
// RuntimeConfig
// ---------------------------------------------------------------------------

/// Unified runtime configuration.
///
/// This is the single source of truth that the runtime pipeline reads
/// to decide how to execute a work order. Construct it from parsed TOML
/// via [`RuntimeConfig::from_backplane_values`], or use the builder pattern.
///
/// ```
/// use abp_runtime::config_integration::{RuntimeConfig, BackendSelectionStrategy};
///
/// let config = RuntimeConfig::builder()
///     .selection_strategy(BackendSelectionStrategy::Projection)
///     .build();
/// assert_eq!(config.selection_strategy, BackendSelectionStrategy::Projection);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// How to select a backend when none is specified.
    pub selection_strategy: BackendSelectionStrategy,
    /// Default policy applied when the work order does not carry one.
    pub default_policy: PolicyProfile,
    /// Telemetry settings.
    pub telemetry: TelemetrySettings,
    /// Workspace staging options.
    pub workspace: WorkspaceOptions,
    /// Global timeout for a single run.
    /// `None` means no timeout.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "optional_duration_ms"
    )]
    pub run_timeout: Option<Duration>,
    /// Maximum number of concurrent runs (0 = unlimited).
    pub max_concurrent_runs: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            selection_strategy: BackendSelectionStrategy::default(),
            default_policy: PolicyProfile::default(),
            telemetry: TelemetrySettings::default(),
            workspace: WorkspaceOptions::default(),
            run_timeout: None,
            max_concurrent_runs: 0,
        }
    }
}

impl RuntimeConfig {
    /// Create a builder for [`RuntimeConfig`].
    #[must_use]
    pub fn builder() -> RuntimeConfigBuilder {
        RuntimeConfigBuilder(Self::default())
    }

    /// Populate a [`RuntimeConfig`] from raw backplane config values.
    ///
    /// This is the bridge between the TOML/env config layer and the runtime.
    #[must_use]
    pub fn from_backplane_values(
        default_backend: Option<&str>,
        workspace_dir: Option<&str>,
        log_level: Option<&str>,
    ) -> Self {
        let selection_strategy = match default_backend {
            Some(name) => BackendSelectionStrategy::Fixed {
                name: name.to_string(),
            },
            None => BackendSelectionStrategy::default(),
        };

        let workspace = WorkspaceOptions {
            base_dir: workspace_dir.map(PathBuf::from),
            ..WorkspaceOptions::default()
        };

        let telemetry = TelemetrySettings {
            tracing_enabled: log_level.map_or(true, |l| l != "off"),
            ..TelemetrySettings::default()
        };

        Self {
            selection_strategy,
            workspace,
            telemetry,
            ..Self::default()
        }
    }

    /// Return `true` if the run should be timed out.
    #[must_use]
    pub fn has_timeout(&self) -> bool {
        self.run_timeout.is_some()
    }

    /// Return `true` if concurrency is limited.
    #[must_use]
    pub fn is_concurrent_limited(&self) -> bool {
        self.max_concurrent_runs > 0
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Builder for [`RuntimeConfig`].
pub struct RuntimeConfigBuilder(RuntimeConfig);

impl RuntimeConfigBuilder {
    /// Set the backend selection strategy.
    #[must_use]
    pub fn selection_strategy(mut self, s: BackendSelectionStrategy) -> Self {
        self.0.selection_strategy = s;
        self
    }

    /// Set the default policy profile.
    #[must_use]
    pub fn default_policy(mut self, p: PolicyProfile) -> Self {
        self.0.default_policy = p;
        self
    }

    /// Set telemetry settings.
    #[must_use]
    pub fn telemetry(mut self, t: TelemetrySettings) -> Self {
        self.0.telemetry = t;
        self
    }

    /// Set workspace options.
    #[must_use]
    pub fn workspace(mut self, w: WorkspaceOptions) -> Self {
        self.0.workspace = w;
        self
    }

    /// Set a global run timeout.
    #[must_use]
    pub fn run_timeout(mut self, d: Duration) -> Self {
        self.0.run_timeout = Some(d);
        self
    }

    /// Set the maximum number of concurrent runs.
    #[must_use]
    pub fn max_concurrent_runs(mut self, n: usize) -> Self {
        self.0.max_concurrent_runs = n;
        self
    }

    /// Consume the builder and produce a [`RuntimeConfig`].
    #[must_use]
    pub fn build(self) -> RuntimeConfig {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Serde helper: Option<Duration> as milliseconds
// ---------------------------------------------------------------------------

mod optional_duration_ms {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(v: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(d) => s.serialize_u64(d.as_millis() as u64),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        let ms: Option<u64> = Option::deserialize(d)?;
        Ok(ms.map(Duration::from_millis))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- BackendSelectionStrategy --

    #[test]
    fn default_strategy_is_fixed_mock() {
        let s = BackendSelectionStrategy::default();
        assert_eq!(
            s,
            BackendSelectionStrategy::Fixed {
                name: "mock".into()
            }
        );
    }

    #[test]
    fn strategy_serde_fixed() {
        let s = BackendSelectionStrategy::Fixed {
            name: "openai".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: BackendSelectionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn strategy_serde_projection() {
        let s = BackendSelectionStrategy::Projection;
        let json = serde_json::to_string(&s).unwrap();
        let back: BackendSelectionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn strategy_serde_fallback() {
        let s = BackendSelectionStrategy::Fallback {
            chain: vec!["a".into(), "b".into()],
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: BackendSelectionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    // -- TelemetrySettings --

    #[test]
    fn default_telemetry() {
        let t = TelemetrySettings::default();
        assert!(t.metrics_enabled);
        assert!(t.tracing_enabled);
        assert!(t.log_interval_runs.is_none());
    }

    #[test]
    fn telemetry_serde_roundtrip() {
        let t = TelemetrySettings {
            metrics_enabled: false,
            tracing_enabled: true,
            log_interval_runs: Some(10),
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: TelemetrySettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    // -- WorkspaceOptions --

    #[test]
    fn default_workspace_options() {
        let w = WorkspaceOptions::default();
        assert!(w.base_dir.is_none());
        assert!(w.git_init);
        assert!(w.baseline_commit);
    }

    #[test]
    fn workspace_options_serde_roundtrip() {
        let w = WorkspaceOptions {
            base_dir: Some(PathBuf::from("/tmp/ws")),
            git_init: false,
            baseline_commit: false,
        };
        let json = serde_json::to_string(&w).unwrap();
        let back: WorkspaceOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(back, w);
    }

    // -- RuntimeConfig --

    #[test]
    fn default_runtime_config() {
        let cfg = RuntimeConfig::default();
        assert!(!cfg.has_timeout());
        assert!(!cfg.is_concurrent_limited());
    }

    #[test]
    fn builder_sets_strategy() {
        let cfg = RuntimeConfig::builder()
            .selection_strategy(BackendSelectionStrategy::Projection)
            .build();
        assert_eq!(cfg.selection_strategy, BackendSelectionStrategy::Projection);
    }

    #[test]
    fn builder_sets_timeout() {
        let cfg = RuntimeConfig::builder()
            .run_timeout(Duration::from_secs(30))
            .build();
        assert!(cfg.has_timeout());
        assert_eq!(cfg.run_timeout.unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn builder_sets_concurrency() {
        let cfg = RuntimeConfig::builder().max_concurrent_runs(4).build();
        assert!(cfg.is_concurrent_limited());
        assert_eq!(cfg.max_concurrent_runs, 4);
    }

    #[test]
    fn builder_sets_telemetry() {
        let t = TelemetrySettings {
            metrics_enabled: false,
            ..TelemetrySettings::default()
        };
        let cfg = RuntimeConfig::builder().telemetry(t.clone()).build();
        assert!(!cfg.telemetry.metrics_enabled);
    }

    #[test]
    fn builder_sets_workspace() {
        let w = WorkspaceOptions {
            git_init: false,
            ..WorkspaceOptions::default()
        };
        let cfg = RuntimeConfig::builder().workspace(w).build();
        assert!(!cfg.workspace.git_init);
    }

    #[test]
    fn builder_sets_default_policy() {
        let mut p = PolicyProfile::default();
        p.allowed_tools.push("bash".into());
        let cfg = RuntimeConfig::builder().default_policy(p.clone()).build();
        assert_eq!(cfg.default_policy.allowed_tools, vec!["bash".to_string()]);
    }

    #[test]
    fn from_backplane_values_with_backend() {
        let cfg = RuntimeConfig::from_backplane_values(Some("openai"), None, None);
        assert_eq!(
            cfg.selection_strategy,
            BackendSelectionStrategy::Fixed {
                name: "openai".into()
            }
        );
    }

    #[test]
    fn from_backplane_values_no_backend() {
        let cfg = RuntimeConfig::from_backplane_values(None, None, None);
        assert_eq!(cfg.selection_strategy, BackendSelectionStrategy::default());
    }

    #[test]
    fn from_backplane_values_workspace_dir() {
        let cfg = RuntimeConfig::from_backplane_values(None, Some("/custom/ws"), None);
        assert_eq!(
            cfg.workspace.base_dir,
            Some(PathBuf::from("/custom/ws"))
        );
    }

    #[test]
    fn from_backplane_values_log_off_disables_tracing() {
        let cfg = RuntimeConfig::from_backplane_values(None, None, Some("off"));
        assert!(!cfg.telemetry.tracing_enabled);
    }

    #[test]
    fn from_backplane_values_log_debug_enables_tracing() {
        let cfg = RuntimeConfig::from_backplane_values(None, None, Some("debug"));
        assert!(cfg.telemetry.tracing_enabled);
    }

    #[test]
    fn runtime_config_serde_roundtrip() {
        let cfg = RuntimeConfig::builder()
            .selection_strategy(BackendSelectionStrategy::Fallback {
                chain: vec!["a".into(), "b".into()],
            })
            .run_timeout(Duration::from_secs(60))
            .max_concurrent_runs(8)
            .build();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.selection_strategy, cfg.selection_strategy);
        assert_eq!(back.run_timeout, cfg.run_timeout);
        assert_eq!(back.max_concurrent_runs, cfg.max_concurrent_runs);
    }

    #[test]
    fn runtime_config_serde_no_timeout() {
        let cfg = RuntimeConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(!json.contains("run_timeout"));
        let back: RuntimeConfig = serde_json::from_str(&json).unwrap();
        assert!(back.run_timeout.is_none());
    }
}
