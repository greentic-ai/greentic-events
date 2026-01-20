# Live E2E Command Protocol

`greentic-events-test e2e --live` delegates ingest/emit/subscription checks to external commands.
Provide the command paths via:

- `GREENTIC_EVENTS_E2E_LIVE_BRIDGE_CMD` (ingest/emit)
- `GREENTIC_EVENTS_E2E_LIVE_BROKER_CMD` (subscriptions)

Each command reads a JSON request from stdin and replies with JSON on stdout.
Non-zero exit codes or invalid JSON are treated as failures.

## Common fields
- `op` (string): operation name.
- `pack_id` (string): pack id (from the pack manifest).
- `provider` (string): provider name from the pack.

## Ingest (bridge command)
Request:
```json
{
  "op": "ingest",
  "pack_id": "events-github",
  "provider": "github",
  "message": { "...": "ChannelMessageEnvelope" }
}
```

Response:
```json
{
  "events": [ { "...": "EventEnvelope" } ]
}
```

## Emit (bridge command)
Request:
```json
{
  "op": "emit",
  "pack_id": "events-github",
  "provider": "github",
  "event": { "...": "EventEnvelope" }
}
```

Response:
```json
{
  "messages": [ { "...": "ChannelMessageEnvelope" } ]
}
```

## Subscriptions (broker command)
Subscribe request:
```json
{
  "op": "subscribe",
  "pack_id": "events-github",
  "provider": "github",
  "topic": "greentic.events.test",
  "options": { "durable": false, "deliver_existing": false, "ack_mode": "auto" }
}
```

Subscribe response:
```json
{
  "subscription_id": "sub-123"
}
```

Publish request:
```json
{
  "op": "publish",
  "pack_id": "events-github",
  "provider": "github",
  "event": { "...": "EventEnvelope" }
}
```

Next-event request:
```json
{
  "op": "next_event",
  "pack_id": "events-github",
  "provider": "github",
  "subscription_id": "sub-123",
  "timeout_ms": 500
}
```

Next-event response:
```json
{
  "event": { "...": "EventEnvelope" }
}
```

Ack request (auto-ack is still sent for validation):
```json
{
  "op": "ack_event",
  "pack_id": "events-github",
  "provider": "github",
  "subscription_id": "sub-123",
  "event_id": "evt-1"
}
```
