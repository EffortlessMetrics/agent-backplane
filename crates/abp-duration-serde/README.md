# abp-duration-serde

Tiny SRP microcrate with reusable serde modules for serializing
`std::time::Duration` values as millisecond integers.

## Modules

- `duration_millis`: for `Duration` fields with `#[serde(with = "...")]`
- `option_duration_millis`: for `Option<Duration>` fields with `#[serde(with = "...")]`
