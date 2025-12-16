# Greentic Events

Multi-tenant event fabric core for Greentic. This crate wires the shared `greentic-types::EventEnvelope` to provider/bridge registries, retry/DLQ handling, and pack-driven discovery.

## Features
- Publish/subscribe API with topic→provider routing.
- Reliability: configurable retry/backoff (fixed, exponential, jitter) with `retryable_errors`, DLQ publishing, idempotency via metadata `idempotency_key`.
- ACLs: topic/tenant allowlists.
- Observability: tracing spans around publish/subscribe, metrics counters/histogram hooks.
- Pack integration: reads `events.providers` from pack v1; optional overrides via annotations:
  - `events.providers.<name>.dlq_topic`
  - `events.providers.<name>.retry.max_retries`
  - `events.providers.<name>.retry.base_delay_ms`
  - `events.providers.<name>.retry.max_delay_ms`
  - `events.providers.<name>.retry.strategy` (`fixed`|`exponential`|`exponential_with_jitter`)
  - `events.providers.<name>.retry.retryable_errors` (array of substrings)
- Bridge registry for message↔event conversions.
- Runner/deployer ready: factories stay runner-agnostic while plugging cleanly into the Greentic runner and deployer services (no protocol/client baked in).

To plug into a runner client later, replace the in-memory factories with your client wrappers when constructing `EventBusBuilder` or the CLI (`greentic-events-cli`).

## Config (pack annotations)
Optional overrides can be set in `pack.yaml` annotations:

- `events.providers.<name>.dlq_topic`
- `events.providers.<name>.retry.max_retries`
- `events.providers.<name>.retry.base_delay_ms`
- `events.providers.<name>.retry.max_delay_ms`
- `events.providers.<name>.retry.strategy` (`fixed`|`exponential`|`exponential_with_jitter`)
- `events.providers.<name>.retry.retryable_errors` (array of substrings)

## CLI
- Inspect providers from a pack: `cargo run --bin greentic-events-cli -- --pack path/to/pack`
- Helper script: `./scripts/inspect_pack.sh tests/fixtures/packs/events_fake`

## Usage
```rust
use greentic_events::{EventBusBuilder, SubscriptionOptions};
use greentic_events::provider::EventProviderFactory;
use futures::StreamExt;
use serde_json::json;
use greentic_types::TenantCtx;

// Implement EventProviderFactory (or use runner-backed factories) and register from a pack:
let factory = MyProviderFactory::new();
let bus = EventBusBuilder::new()
    .register_from_pack_dir(pack_path, &factory, None)
    .await?
    .build();

// Publish
bus.publish(event_envelope).await?;

// Subscribe
let mut handle = bus.subscribe("topic.*", tenant_ctx, SubscriptionOptions::default()).await?;
if let Some(event) = handle.next().await { /* ... */ }
```

### Minimal publish/subscribe API (domain-agnostic)
If you just need a generic JSON payload on a string topic, use the helper that builds the envelope for you:
```rust
use greentic_events::{EventBusBuilder, SubscriptionOptions};
use greentic_events::provider::EventProviderFactory;
use serde_json::json;
use greentic_types::TenantCtx;

let tenant = TenantCtx::new("dev".try_into()?, "tenant-a".try_into()?);
let bus = EventBusBuilder::new()
    // register providers directly or via `register_from_pack_dir(...)`
    .register_provider(my_provider_registration)
    .build();

bus.publish_event(&tenant, "greentic.repo.build.status", json!({
    "status": "succeeded",
    "build_id": "b-123",
}))
.await?;

let mut sub = bus
    .subscribe_topic("greentic.repo.build.status", &tenant, SubscriptionOptions::default())
    .await?;
if let Some(evt) = sub.next().await {
    assert_eq!(evt.topic, "greentic.repo.build.status");
    assert_eq!(evt.payload["status"], "succeeded");
}
```
`subscribe_topic` simply clones the provided tenant context for convenience; full `subscribe` remains available.

### Topic conventions (recommendations, not enforced)
- `greentic.repo.build.status`
- `greentic.repo.publish.completed`
- `greentic.repo.component.deprecated`
- `greentic.repo.component.recalled`
These stay string-based only; greentic-events does not ship typed repo event enums.

See `docs/secrets-events.md` for recommended secrets topic patterns and example payloads (redacted metadata only; no secret values).

## Default providers (OCI)
`greentic-events-providers` is published as an OCI artifact at `oci://ghcr.io/greentic-ai/greentic-events-providers:latest`. Pull it with your OCI client (for example, `oras pull ghcr.io/greentic-ai/greentic-events-providers:latest -o /tmp/greentic-events-providers`) and point the CLI or your host at that directory:

```
oras pull ghcr.io/greentic-ai/greentic-events-providers:latest -o /tmp/greentic-events-providers
cargo run --bin greentic-events-cli -- --pack /tmp/greentic-events-providers
```
- Optional helper: `./scripts/pull_providers.sh [dest_dir]` uses `oras` to fetch the pack locally for quick inspection. This lives at the scripting/CLI layer so the core crate remains transport-agnostic.

## Runner integration hooks
See `src/runner.rs` for adapter structs you can back with the Greentic runner to construct providers or bridges from WIT components. These remain thin wrappers: you supply the invocation functions; no Wasm glue lives here. Pair them with the runner/deployer services to spin up providers declared in packs. The core library does not ship a runner/deployer client or pick a transport; hosts inject their own bindings.

## Testing
`cargo test` (Rust 1.89). Fixtures live under `tests/fixtures/packs`. CI sets `CLI_SMOKE=1` to ensure the CLI inspector keeps working.

## CI & Publishing
- Run `ci/local_check.sh` before pushing (fmt, clippy -D warnings, tests).
- GitHub Actions `.github/workflows/ci.yml` runs the same checks.
- `.github/workflows/publish.yml` is wired for crates.io publishing via `CARGO_REGISTRY_TOKEN` but currently skips because this crate is `publish = false`; flip that and remove the skip when ready to publish.
