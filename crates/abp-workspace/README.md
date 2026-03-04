# abp-workspace

Staged workspace creation with glob filtering for Agent Backplane.

Prepares a working directory for agent execution. Supports two modes:

- **PassThrough** — use the workspace as-is
- **Staged** — copy files into a temp directory (with include/exclude globs),
  auto-initialize a git repo, and create a baseline commit for meaningful diffs

## Key Types

| Type | Description |
|------|-------------|
| `WorkspaceManager` | Entry point for workspace preparation |
| `PreparedWorkspace` | Ready-to-use workspace, potentially backed by a temp directory |
| `WorkspaceStager` | Fluent builder for staged workspace creation |

## Usage

```rust,no_run
use abp_workspace::WorkspaceManager;

// WorkspaceManager is a unit struct — call methods directly
// let prepared = WorkspaceManager::prepare(&spec).await?;
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
