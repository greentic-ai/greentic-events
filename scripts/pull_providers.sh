#!/usr/bin/env bash
set -euo pipefail

IMAGE="${IMAGE:-ghcr.io/greentic-ai/greentic-events-providers:latest}"
DEST="${1:-./dist/greentic-events-providers}"

if ! command -v oras >/dev/null 2>&1; then
  echo "oras is required but not found. Install oras (https://oras.land) and rerun." >&2
  exit 1
fi

mkdir -p "${DEST}"
echo "Pulling ${IMAGE} into ${DEST} ..."
oras pull "${IMAGE}" -o "${DEST}"
echo "Done. Inspect with: cargo run --bin greentic-events-cli -- --pack ${DEST}"
