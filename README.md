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
- Runner-agnostic: factories are injection points; no default runner client is bundled. Wire them from your host or a future `greentic-runner-client`.

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

## Runner integration hooks
See `src/runner.rs` for adapter structs you can back with the Greentic runner to construct providers or bridges from WIT components. These remain thin wrappers: you supply the invocation functions; no Wasm glue lives here.

## Testing
`cargo test` (Rust 1.89). Fixtures live under `tests/fixtures/packs`.

## CI & Publishing
- Run `ci/local_check.sh` before pushing (fmt, clippy -D warnings, tests).
- GitHub Actions `.github/workflows/ci.yml` runs the same checks.
- `.github/workflows/publish.yml` is wired for crates.io publishing via `CARGO_REGISTRY_TOKEN` but currently skips because this crate is `publish = false`; flip that and remove the skip when ready to publish.
