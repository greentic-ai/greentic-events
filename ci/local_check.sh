#!/usr/bin/env bash
set -euo pipefail

# From repo root
cargo fmt --all

cargo clippy --workspace --all-targets --all-features -- -D warnings

./scripts/events_provider_doctor.sh
./scripts/events_provider_smoke.sh

cargo test --workspace --all-features
