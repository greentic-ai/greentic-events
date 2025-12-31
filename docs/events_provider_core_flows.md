# Provider-core events flow template

This repo ships a reusable flow template for invoking the provider-core `publish` operation without baking in a transport-specific provider.

- Template: `flows/events/publish_event.flow.yaml`
- Provider-agnostic: pick `provider_id` / `provider_type` at runtime (for example `events.webhook`, `events.timer`, `events.sms`, `events.email`).
- Contract: the request payload matches `schemas/events/ops/publish.input.schema.json`.

## How the template works
- Flow kind: `event`; entrypoint: `default`.
- Single node `publish` using component `greentic.provider.invoke` with `operation: publish`.
- Input mapping builds the publish request:
  - `provider_id` / `provider_type` pulled from state (set by UI/config/defaults).
  - `request.topic` from `payload.topic`.
  - `request.event`: payload, optional `id`/`type`/`ts`, plus meta/metadata/idempotency key.
  - `request.options`: `dedupe_key`, `ttl_seconds`, `priority`.
- Output mapping stores:
  - `state.last_receipt_id`, `state.last_provider_event_id`
  - `payload.publish_result` (full publish output).

## Swapping providers at runtime
- Set `state.provider_id` and `state.provider_type` before invoking the flow (for example via configurator, UI form, or defaults).
- No other fields need to change; the same request body is sent to any provider that implements `events.publish`.

## Mapping guidance
- Minimum required fields: `topic` and `event.payload`.
- Optional hints: `event.type`, `event.ts`/`event.time` (RFC3339), `event.meta.tenant`, `event.meta.tags`, `options.dedupe_key`, `options.ttl_seconds`, `options.priority`.
- Keep `event.topic` in sync with the top-level `topic` (the template enforces this via mapping).
- The template sets timestamps via `payload.event_ts` or falls back to `now_rfc3339` (host helper).

## Topic conventions
- Use dotted, lowercased subjects: `orders.created`, `billing.invoice.sent`, `greentic.repo.build.status`.
- Derive DLQ names as `<topic>.dlq` unless the provider overrides.
- Keep provider types namespaced: `events.webhook`, `events.timer`, `events.sms`, `events.email`.

## Extending the template
- Add routing after the `publish` node (for example branching on `publish_result.status`).
- Prepend a validation node if you want to reject missing topics/payloads before calling the provider.
- Attach telemetry tags via flow/entrypoint metadata if your runner supports it.
