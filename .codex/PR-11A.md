# PR-11A.md (greentic-events)
# Title: Define provider-core events operation contracts (JSON schemas + shared types)

## Goal
Make `greentic-events` the authoritative place for:
- Event envelope JSON shapes (aligned with `greentic-types::events` envelope)
- Provider-core operation contracts (input/output JSON schemas) for event ops like `publish` (and later `subscribe`/`ack`)
- Shared helpers for validation/normalization across providers

WITHOUT:
- implementing provider-specific transports (webhook/sms/timer/email) here
- introducing any new WIT provider protocols

## Non-goals
- Do not add provider implementations (webhook/sms/timer/email) in this repo.
- Do not depend on `greentic-interfaces-guest`; use `greentic-types` + JSON schema only.
- Do not create a second provider mechanism.

## Deliverables
1) Canonical JSON Schemas:
- `schemas/events/common/event_envelope.schema.json`
- `schemas/events/ops/publish.input.schema.json`
- `schemas/events/ops/publish.output.schema.json`
Optional (future, but scaffold now if helpful):
- `schemas/events/ops/subscribe.input.schema.json`
- `schemas/events/ops/subscribe.output.schema.json`
- `schemas/events/ops/ack.input.schema.json`
- `schemas/events/ops/ack.output.schema.json`

2) Rust types matching the schemas (recommended):
- `src/provider_ops.rs`:
  - `PublishInput`, `PublishOutput`
  - `EventEnvelope` (wrap or reuse from `greentic-types` if available)
- `src/validate.rs`:
  - `validate_publish_input(&PublishInput) -> Result<(), ValidationIssue>`

3) Docs:
- `docs/provider_core_events_ops.md` describing:
  - op names and semantics
  - topic naming guidance
  - minimum required fields
  - receipt/ids semantics
  - provider_type naming (events.webhook, events.timer, events.sms, events.email)

## Operation contract (minimum)
### invoke("publish")
Input JSON:
{
  "topic": "string",                 // logical topic/subject
  "event": {
    "id": "string"?,                 // optional; provider may assign
    "ts": "RFC3339 timestamp"?,
    "type": "string"?,               // e.g. "order.created"
    "payload": { ... },              // arbitrary JSON
    "meta": { "tenant": "...", "tags": ["..."] }?
  },
  "options": {
    "dedupe_key": "string"?,
    "ttl_seconds": 3600?,
    "priority": "low|normal|high"?
  }?
}

Output JSON:
{
  "receipt_id": "string",            // stable receipt id
  "provider_event_id": "string"?,    // provider native id, if any
  "status": "published|queued",
  "ts": "RFC3339 timestamp"?
}

## Tests
- JSON schema validity tests
- Fixtures validate against schemas:
  - `tests/fixtures/publish_input.json`
  - `tests/fixtures/publish_output.json`
- Serde round-trip tests for Rust types (if added)

## Acceptance criteria
- Schemas exist and are stable.
- Providers can implement `publish` op without guessing payloads.
- No provider-specific code added.
