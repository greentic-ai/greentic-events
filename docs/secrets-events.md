# Secrets Event Topics

This crate does not enforce schemas, but these conventions keep secrets-related events consistent across services.

## Topics
- `greentic.secrets.put`
- `greentic.secrets.delete`
- `greentic.secrets.rotate.requested`
- `greentic.secrets.rotate.completed`
- `greentic.secrets.missing.detected`

## Payload principles
- JSON payloads only; include an optional `schema_version` (example: `"v1"`) for evolution.
- Never include secret bytes or values (even base64-encoded). Payloads carry metadata, not secrets.
- Prefer including `correlation_id` / `flow_id` / `pack_id` when available.
- Include tenant/session identity you already have (for example, `tenant_ctx`, `session_id`).
- Timestamps should be RFC 3339 strings.

## Examples (non-normative)

### greentic.secrets.put
```json
{
  "schema_version": "v1",
  "key": "services/payments/api-key",
  "scope": { "env": "prod", "tenant": "acme", "team": "payments" },
  "tenant_ctx": { "environment": "prod", "tenant": "acme" },
  "correlation_id": "flow-123",
  "pack_id": "billing-pack@1.2.3",
  "result": "success",
  "timestamp": "2024-12-18T12:34:56Z",
  "metadata": { "issuer": "greentic-secrets-cli" }
}
```

### greentic.secrets.delete
```json
{
  "schema_version": "v1",
  "key": "services/payments/api-key",
  "scope": { "env": "prod", "tenant": "acme", "team": "payments" },
  "tenant_ctx": { "environment": "prod", "tenant": "acme" },
  "correlation_id": "flow-456",
  "pack_id": "billing-pack@1.2.3",
  "result": "success",
  "timestamp": "2024-12-18T12:40:00Z",
  "metadata": { "issuer": "greentic-secrets-cli" }
}
```

### greentic.secrets.rotate.requested
```json
{
  "schema_version": "v1",
  "key": "services/payments/api-key",
  "scope": { "env": "prod", "tenant": "acme", "team": "payments" },
  "tenant_ctx": { "environment": "prod", "tenant": "acme" },
  "correlation_id": "flow-789",
  "pack_id": "billing-pack@1.2.3",
  "requested_by": "user-123",
  "reason": "scheduled-rotation",
  "timestamp": "2024-12-18T12:45:00Z"
}
```

### greentic.secrets.rotate.completed
```json
{
  "schema_version": "v1",
  "key": "services/payments/api-key",
  "scope": { "env": "prod", "tenant": "acme", "team": "payments" },
  "tenant_ctx": { "environment": "prod", "tenant": "acme" },
  "correlation_id": "flow-789",
  "pack_id": "billing-pack@1.2.3",
  "result": "success",
  "timestamp": "2024-12-18T12:46:10Z",
  "metadata": { "rotation_window": "5m" }
}
```

### greentic.secrets.missing.detected
```json
{
  "schema_version": "v1",
  "key": "services/payments/api-key",
  "scope": { "env": "prod", "tenant": "acme", "team": "payments" },
  "tenant_ctx": { "environment": "prod", "tenant": "acme" },
  "correlation_id": "flow-999",
  "pack_id": "billing-pack@1.2.3",
  "result": "missing",
  "error": "secret not found in store",
  "timestamp": "2024-12-18T12:50:00Z"
}
```

## Redaction rule
These events must never include secret values, even if base64-encoded. Emit only metadata that helps operators triage (key, scope, requester, result/error, correlation identifiers).
