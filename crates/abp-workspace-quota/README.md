# abp-workspace-quota

Single-responsibility microcrate for workspace disk-usage quota enforcement.

It provides:

- Workspace usage checks (`WorkspaceQuota::check`)
- Over-quota detection (`WorkspaceQuota::is_exceeded`)
- Best-effort file cleanup to reclaim space (`WorkspaceQuota::cleanup`)

The crate excludes `.git` directories from size calculations and cleanup.
