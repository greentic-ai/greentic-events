#!/usr/bin/env bash
set -euo pipefail

PROVIDER_NAME="greentic-events-providers"
PACK_ROOT=""
OCI_REF=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --provider)
      PROVIDER_NAME="$2"
      shift 2
      ;;
    --pack-root|--pack-dir)
      PACK_ROOT="$2"
      shift 2
      ;;
    --oci-ref|--ref)
      OCI_REF="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

DEFAULT_ROOT="./providers"
if [[ ! -d "${DEFAULT_ROOT}" && -d "./packs" ]]; then
  DEFAULT_ROOT="./packs"
fi

PACK_ROOT="${PACK_ROOT:-${DEFAULT_ROOT}}"
DEST_DIR="${PACK_ROOT%/}/${PROVIDER_NAME}"
OCI_REF="${OCI_REF:-ghcr.io/greentic-ai/${PROVIDER_NAME}:latest}"

if ! command -v oras >/dev/null 2>&1; then
  echo "oras is required but missing; install it (https://oras.land) and retry." >&2
  exit 1
fi

echo "Fetching provider pack ${PROVIDER_NAME} from ${OCI_REF}"
mkdir -p "${DEST_DIR}"
oras pull "${OCI_REF}" -o "${DEST_DIR}"

echo "Pack available at ${DEST_DIR}"
echo "Inspect with: cargo run --bin greentic-events-cli -- --pack \"${DEST_DIR}\""
