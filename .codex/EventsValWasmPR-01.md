# EventsValWasmPR-01 — Build and publish events pack validator as WASM + declare it in event extensions

**Repo:** `greentic-ai/greentic-events`

## Goal
Create an **events domain validator** as a WASM component implementing `greentic:pack-validate@0.1.0`, publish to OCI, and define how events packs/extensions reference it.

## Deliverables
- `crates/greentic-events-pack-validator` (WASM component)
- `validators/events/` pack producing `dist/validators-events.gtpack`
- CI publish to `ghcr.io/greentic-ai/validators/events:<ver>`
- Docs: `docs/pack-validation.md`

## Validator rules (minimal)
- `EVT_SCHEMA_MISSING`: schemas referenced by manifest/providers missing from pack (file_index)
- `EVT_INVALID_TOPIC`: invalid topic naming when declared (if present in annotations/manifest)
- `EVT_ENTRY_FLOW_MISSING`: entry flow in meta.entry_flows missing from flows list
- `EVT_HANDLER_FLOW_MISSING`: handler flow declared by convention missing (Warn ok)

## Inputs
Use only manifest_cbor + sbom_json + file_index.

## Acceptance criteria
- Builds, packages, and can be executed by greentic-pack doctor once validator support is merged.
