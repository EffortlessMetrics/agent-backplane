# abp-budget

Budget tracking primitives for Agent Backplane.

This crate exposes:

- `BudgetLimit` for configuring per-run token/cost/turn/duration caps
- `BudgetTracker` for lock-free accounting across concurrent callers
- `BudgetStatus`, `BudgetViolation`, and `BudgetRemaining` for budget checks
