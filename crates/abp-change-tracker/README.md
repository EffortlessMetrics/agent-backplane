# abp-change-tracker

Small, focused types for tracking file-level changes and computing aggregate summaries.

This crate intentionally contains only change-tracking domain logic so it can be shared by
multiple workspace or runtime components without pulling in staging-specific behavior.
