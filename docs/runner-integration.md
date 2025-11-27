# Runner integration

This crate stays WASM-agnostic; you connect it to the Greentic runner (or any component host) via the adapters in `src/runner.rs`.

> Note: There is no official greentic-runner client crate yet. The adapters here are injection points; wire them from your host once a client/API is available.

## Wiring providers via runner

```rust
use std::sync::Arc;
use greentic_events::bus::EventBusBuilder;
use greentic_events::runner::RunnerProviderFactory;
use greentic_runner_client::EventsInvoker; // hypothetical runner client

let invoker = EventsInvoker::new(/* transport to runner host */);
let provider_factory = RunnerProviderFactory {
    build: Arc::new(move |spec| {
        // Use your runner client to instantiate the provider component declared in the pack.
        invoker.build_provider(spec)
    }),
};

let bus = EventBusBuilder::new()
    .register_from_pack_dir(pack_dir, &provider_factory, None)
    .await?
    .build();
```

## Wiring bridges via runner

```rust
use greentic_events::runner::RunnerBridgeFactory;

let bridge_factory = RunnerBridgeFactory {
    message_to_event: Arc::new(move |name| runner_invoker.build_msg_to_event(name)),
    event_to_message: Arc::new(move |name| runner_invoker.build_event_to_msg(name)),
};

let bus = EventBusBuilder::new()
    .register_from_pack_dir(pack_dir, &provider_factory, Some(&bridge_factory))
    .await?
    .build();
```

## CLI example

```
# Use the helper script (defaults to fixtures)
./scripts/inspect_pack.sh tests/fixtures/packs/events_fake

# Or directly:
cargo run --bin greentic-events-cli -- --pack path/to/pack
```

Replace `greentic_runner_client` with your actual runner host bindings; the adapters simply require closures that return `Arc<dyn EventProvider>` or bridge instances.
