#!/usr/bin/env bash
set -euo pipefail

# From repo root
cargo fmt --all

cargo clippy --workspace --all-targets --all-features -- -D warnings

cargo test --workspace --all-features
