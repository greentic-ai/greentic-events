#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PACK_DIR="$ROOT_DIR/validators/events"
COMPONENTS_DIR="$PACK_DIR/components"
DIST_DIR="$PACK_DIR/dist"

mkdir -p "$COMPONENTS_DIR" "$DIST_DIR"

cargo build -p greentic-events-pack-validator --target wasm32-wasip2 --release

cp "$ROOT_DIR/target/wasm32-wasip2/release/greentic_events_pack_validator.wasm" \
  "$COMPONENTS_DIR/greentic-events-pack-validator.wasm"

greentic-pack build \
  --in "$PACK_DIR" \
  --gtpack-out "$DIST_DIR/validators-events.gtpack" \
  --no-update
