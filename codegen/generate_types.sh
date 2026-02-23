#!/usr/bin/env bash
set -euo pipefail

# Example-only. You will likely replace this with a pinned, reproducible toolchain.

cargo run -p xtask -- schema

echo "Schemas generated under contracts/schemas/"

echo "Now generate TypeScript / Python types using your preferred generator."
