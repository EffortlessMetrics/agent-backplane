# abp-projection

Projection matrix that routes work orders to the best-fit backend.

## Overview

`abp-projection` combines the capability negotiation from `abp-capability`,
the cross-dialect mapping quality from `abp-mapping`, and a backend registry
to determine which backend is best suited for a given `WorkOrder`.

## Key types

- **`ProjectionMatrix`** — The central registry. Backends are registered with
  their capabilities, dialect, and priority. Given a `WorkOrder`, the matrix
  scores each backend and returns a `ProjectionResult`.
- **`ProjectionResult`** — Contains the selected backend, its fidelity score,
  required emulations, and a fallback chain of alternative backends.
- **`ProjectionScore`** — Composite score combining capability coverage,
  mapping fidelity, and backend priority.

## Scoring

The projection score is a weighted sum of three factors:

| Factor                | Weight | Description                             |
|-----------------------|--------|-----------------------------------------|
| Capability coverage   | 0.5    | Fraction of required capabilities met   |
| Mapping fidelity      | 0.3    | Fraction of features mapped losslessly  |
| Backend priority      | 0.2    | Normalized priority (higher is better)  |

Backends with unsupported required capabilities are excluded unless no
fully-compatible backend exists, in which case they appear only in the
fallback chain.

## Passthrough mode

When a work order requests passthrough mode (via
`config.vendor["abp"]["mode"] == "passthrough"`), backends whose native
dialect matches the work order's source dialect receive a bonus, making
same-dialect passthrough the preferred path.

## License

MIT OR Apache-2.0
