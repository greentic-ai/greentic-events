# Pack validation diagnostics

This document lists Greentic events-domain validation codes and their meanings. All diagnostics
follow the greentic-types validation model.

## Events domain validators

- `EVT_PROVIDER_SCHEMA_EMPTY`: Provider declarations in events packs must include a non-empty
  `config_schema_ref`.
- `EVT_PROVIDER_NO_OPS`: Provider declarations include zero operations; this is a warning to
  avoid blocking initial schema-only packs.
- `EVT_INVALID_TOPIC`: Event topic annotations must match `^[a-z0-9]+(\.[a-z0-9_-]+)+$`.
- `EVT_SUSPECT_TYPE_FORMAT`: Event type annotations should be non-empty and use reverse-DNS
  prefixes such as `com.`, `org.`, or `ai.greentic.`.
- `EVT_ENTRY_FLOW_MISSING`: Entry flow identifiers declared in manifest metadata are missing
  from `manifest.flows`.
- `EVT_HANDLER_FLOW_MISSING`: Expected handler flows (ids starting with `handle_`) are missing
  or are not `flow-v1` schema entries.

## Metadata conventions used by validators

Validators look for metadata in extension inline payloads. If a pack includes an extension with
`annotations` or `meta` fields, the following keys are considered:

- `annotations.event.topic` or `annotations.events.topic`
- `annotations.event.type` or `annotations.events.type`
- `meta.entry_flows` or top-level `entry_flows` (array of flow id strings)
