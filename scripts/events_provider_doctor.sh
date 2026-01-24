#!/usr/bin/env bash
set -euo pipefail

PROVIDER_NAME=""
PACK_ROOT=""
STRICT_MODE="false"

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
    --strict)
      STRICT_MODE="true"
      shift
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

    while IFS= read -r -d '' pack; do
      PACK_ENTRIES+=("$pack")
    done < <(find "$root" -name "*.gtpack" -print0 2>/dev/null || true)
  elif [[ -f "$root" ]]; then
    PACK_ENTRIES+=("$root")
  fi
done

if [[ ${#PACK_ENTRIES[@]} -eq 0 ]]; then
  echo "No pack directories or gtpack files found in ${ROOTS[*]}; nothing to doctor."
  exit 0
fi

# Deduplicate while respecting ordering
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
  echo "No packs match provider filter '${PROVIDER_NAME}'; nothing to doctor."
  exit 0
fi

if ! command -v greentic-pack >/dev/null 2>&1; then
  echo "greentic-pack is required but not on PATH; cargo install greentic-pack-lib or use cargo run -p greentic-pack." >&2
  exit 1
fi

STRICT_FLAG=""
if [[ "$STRICT_MODE" == "true" ]]; then
  STRICT_FLAG="--strict"
fi

DOCTORED=0
for pack in "${PACK_PATHS[@]}"; do
  echo "Running greentic-pack doctor ${STRICT_FLAG} on ${pack}..."
  greentic-pack doctor ${STRICT_FLAG} "${pack}"
  DOCTORED=$((DOCTORED + 1))
done

echo "Doctored ${DOCTORED} pack(s)."
