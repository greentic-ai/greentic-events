#!/usr/bin/env bash
set -euo pipefail

PROVIDER_NAME=""
PACK_ROOT=""

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
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

ROOTS=()
if [[ -n "$PACK_ROOT" ]]; then
  ROOTS+=("$PACK_ROOT")
else
  ROOTS+=("./providers" "./packs" "tests/fixtures/packs")
fi

PACK_ENTRIES=()

for root in "${ROOTS[@]}"; do
  if [[ -d "$root" ]]; then
    while IFS= read -r -d '' pack; do
      PACK_ENTRIES+=("$(dirname "$pack")")
    done < <(find "$root" -name "pack.yaml" -print0 2>/dev/null || true)
  elif [[ -f "$root" ]]; then
    PACK_ENTRIES+=("$(dirname "$root")")
  fi
done

if [[ ${#PACK_ENTRIES[@]} -eq 0 ]]; then
  echo "No pack directories (with pack.yaml) found in ${ROOTS[*]}; skipping smoke checks."
  exit 0
fi

readarray -t PACK_PATHS < <(printf "%s\n" "${PACK_ENTRIES[@]}" | sort -u)

if [[ -n "$PROVIDER_NAME" ]]; then
  FILTERED=()
  for path in "${PACK_PATHS[@]}"; do
    if [[ "$path" == *"$PROVIDER_NAME"* ]]; then
      FILTERED+=("$path")
    fi
  done
  PACK_PATHS=("${FILTERED[@]}")
fi

if [[ ${#PACK_PATHS[@]} -eq 0 ]]; then
  echo "No packs match provider filter '${PROVIDER_NAME}'; skipping smoke checks."
  exit 0
fi

for pack in "${PACK_PATHS[@]}"; do
  echo "Smoke testing provider pack at ${pack}"
  RUST_LOG=info cargo run --bin greentic-events-cli -- --pack "${pack}"
done

echo "Smoke test completed for ${#PACK_PATHS[@]} pack(s)."
