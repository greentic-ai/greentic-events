# Provider-core events operations

The `events` provider surface exposes JSON contracts for publish/subscribe/ack-style operations without binding to any transport. Canonical schemas live under `schemas/events/**` and the matching Rust types/validators live in `src/provider_ops.rs` and `src/validate.rs`.

## Operations
- `publish`: accept an event envelope plus optional delivery options and return a receipt.
- `subscribe` (scaffold): reserved for streaming contracts.
- `ack` (scaffold): reserved for acknowledgement contracts.

## Event envelope (common)
File: `schemas/events/common/event_envelope.schema.json`

Fields align with `greentic-types::EventEnvelope`, but allow providers to fill in gaps:
- `id` (string, optional): provider may assign when missing.
- `topic` (string): logical subject such as `greentic.repo.build.status`.
- `type` (string, optional): defaults are provider-defined (for example `greentic.events.generic`).
- `source` / `subject` / `correlation_id` (strings, optional).
- `ts` or `time` (RFC3339 string): alias for envelope timestamp; both must match when provided.
- `payload` (any JSON): required.
- `meta` (object): lightweight routing hints such as `tenant` or `tags`; accepts extra keys.
- `metadata` (map<string,string>): idempotency keys and other string-only metadata.
- `tenant` (object, optional): full `TenantCtx` snapshot when available.

## Publish input
File: `schemas/events/ops/publish.input.schema.json`  
Rust: `greentic_events::PublishInput`

Required:
- `topic`: target topic/subject.
- `event.payload`: JSON payload (object/array/primitive).

Optional:
- `event`: `id`, `type`, `source`, `subject`, `ts`/`time`, `meta`, `metadata`, `tenant`.
- `options`: `dedupe_key` (idempotency), `ttl_seconds` (hint), `priority` (`low|normal|high`).

Example:
```json
{
  "topic": "orders.created",
  "event": {
    "id": "evt-12345",
    "ts": "2024-01-02T03:04:05Z",
    "type": "order.created",
    "payload": { "order_id": "ord-1", "total": 25.5 },
    "meta": { "tenant": "tenant-a", "tags": ["orders", "created"] }
  },
  "options": { "dedupe_key": "order-ord-1", "ttl_seconds": 3600, "priority": "normal" }
}
```

Validation helper: `greentic_events::validate_publish_input(&PublishInput)` checks topic alignment, empty/whitespace fields, and RFC3339 timestamps.

## Publish output
File: `schemas/events/ops/publish.output.schema.json`  
Rust: `greentic_events::PublishOutput`

- `receipt_id` (string, required): stable receipt.
- `provider_event_id` (string, optional): native id from the provider.
- `status` (`published|queued`, required).
- `ts` (RFC3339 string, optional): acceptance timestamp.

Example:
```json
{ "receipt_id": "rcpt-abc123", "provider_event_id": "native-789", "status": "published", "ts": "2024-01-02T03:04:06Z" }
```

## Topic and provider naming
- Topics: dotted, lowercase segments; prefer domain-first names (for example `greentic.repo.build.status`, `orders.created`, `billing.invoice.sent`).
- Provider type naming for transports: `events.webhook`, `events.timer`, `events.sms`, `events.email` (no transport-specific code lives here).
- Providers may add DLQ/scheduling semantics, but must honor the shared publish contract above.

## Fixtures and tests
- Fixtures: `tests/fixtures/publish_input.json`, `tests/fixtures/publish_output.json`.
- Tests validate JSON Schemas and serde round-trips in `tests/provider_core_ops.rs`.

