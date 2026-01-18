# Events pack validation (WASM)

This repository ships a WASM component that implements the
`greentic:pack-validate/pack-validator@0.1.0` world for events-domain packs.

## Validator component

- Component crate: `crates/greentic-events-pack-validator`
- Pack source: `validators/events/`
- OCI target: `oci://ghcr.io/greentic-ai/validators/events:<ver>`

## Diagnostics

The events validator emits the following codes:

- `EVT_SCHEMA_MISSING` (error): a provider schema reference points to a file
  not present in the pack file index.
- `EVT_INVALID_TOPIC` (error): a declared event topic does not match
  `^[a-z0-9]+(\.[a-z0-9_-]+)+$`.
- `EVT_ENTRY_FLOW_MISSING` (error): an entry flow listed in metadata is missing
  from `manifest.flows`.
- `EVT_HANDLER_FLOW_MISSING` (warn): a handler flow with `handle_` prefix is
  missing from `manifest.flows`.

## Metadata keys consumed by the validator

When `extensions` include inline metadata, the validator checks:

- `extensions.*.annotations.event.topic` or `extensions.*.annotations.events.topic`
- `extensions.*.meta.entry_flows` or `extensions.*.entry_flows`

## Declaring the validator in events packs

Events packs can reference the validator via a custom extension. The extension
payload is free-form JSON (stored as `ExtensionInline::Other`).

Example manifest extension:

```yaml
extensions:
  greentic.events.validation.v1:
    kind: greentic.events.validation.v1
    version: "1.0.0"
    inline:
      validators:
        - ref: "oci://ghcr.io/greentic-ai/validators/events:<ver>"
          world: "greentic:pack-validate/pack-validator@0.1.0"
```

## Building the validator pack

The helper script expects `greentic-pack` on PATH and the `wasm32-wasip2`
Rust target installed:

```bash
scripts/build_validators_pack.sh
```

This produces `validators/events/dist/validators-events.gtpack`.
