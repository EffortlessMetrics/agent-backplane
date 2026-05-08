setup:
    cargo xtask setup

lint-fix:
    cargo xtask lint-fix

lint-check:
    cargo xtask lint-fix --check

gate:
    cargo xtask gate

gate-check:
    cargo xtask gate --check

test:
    cargo test --workspace

test-compile:
    cargo test --workspace --no-run

schema:
    cargo run -p xtask -- schema

generate-types:
    cargo run -p xtask -- generate-types

fmt-comprehensive-tests:
    cargo run -p xtask -- fmt-comprehensive-tests

check:
    cargo xtask check

audit:
    cargo xtask audit

stats:
    cargo xtask stats

docs:
    cargo xtask docs

docs-open:
    cargo xtask docs --open

close-superseded-prs:
    cargo xtask close-superseded-prs
