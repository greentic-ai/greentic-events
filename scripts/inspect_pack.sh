#!/usr/bin/env bash
set -euo pipefail

PACK_DIR="${1:-tests/fixtures/packs/events_fake}"

echo "Inspecting providers in ${PACK_DIR}"
cargo run --bin greentic-events-cli -- --pack "${PACK_DIR}"
