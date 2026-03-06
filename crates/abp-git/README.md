# abp-git

Git repository helpers for workspace staging and verification in the Agent Backplane.

Provides functions for initializing repos, capturing diffs, computing change
statistics, and creating commits using the ABP identity. Used by workspace
staging to establish a baseline commit and capture agent-produced patches.

## Key Types

| Type | Description |
|------|-------------|
| `ChangeStats` | Aggregated file-level change statistics (added, modified, deleted, line counts) |
| `BlameLine` | A single line of parsed `git blame` output with commit, author, and content |

## Key Functions

| Function | Description |
|----------|-------------|
| `ensure_git_repo` | Initializes a git repo with a baseline commit if none exists |
| `git_diff` | Returns unified diff output for uncommitted changes |
| `git_create_patch` | Creates a unified patch of all workspace changes relative to HEAD |
| `git_change_stats` | Computes file-level change statistics for all modifications |
| `git_commit` | Stages all changes and creates a commit with the ABP identity |
| `git_blame` | Runs porcelain blame and returns parsed per-line results |

## Usage

```rust,no_run
use std::path::Path;
use abp_git::{ensure_git_repo, git_diff, git_change_stats};

let workspace = Path::new("/tmp/staged-workspace");
ensure_git_repo(workspace);

if let Some(diff) = git_diff(workspace) {
    println!("Changes:\n{diff}");
}
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
