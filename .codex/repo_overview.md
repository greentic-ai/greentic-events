# Repository Overview

## 1. High-Level Purpose
- Core event fabric for Greentic, wiring `greentic-types::EventEnvelope` to providers/bridges discovered from packs with routing, ACL enforcement, retries/DLQ handling, idempotency, and telemetry hooks.
- Ships runner-friendly adapters so hosts can supply provider/bridge implementations (WIT/runner clients or custom factories) while remaining transport-agnostic; includes a CLI to inspect provider descriptors from a pack.

## 2. Main Components and Functionality
- **Path:** `src/bus.rs`  
  **Role:** Event bus and builder that coordinate provider routing, ACLs, retries/DLQ, idempotency, metrics, and bridge registration.  
  **Key functionality:** Publish with retry/backoff and DLQ fallback; ACL checks on publish/subscribe; topic→provider resolution via glob matching; registration from pack manifests with annotation overrides; accessors for metrics and bridge service.  
  **Key dependencies / integration points:** Uses `greentic-pack` specs, provider factory implementations, and bridge factories; relies on `IdempotencyStore`, `RetryPolicy`, `TopicAcl`, and `EventMetrics`.
- **Path:** `src/provider.rs`  
  **Role:** Traits and helpers describing event providers and their runtime registration.  
  **Key functionality:** `EventProvider` and `EventProviderFactory` traits; `ProviderRegistration` runtime config; descriptor conversion from pack specs, mapping transport/reliability/ordering to `greentic-types`.  
  **Key dependencies / integration points:** Consumed by `EventBusBuilder::register_from_pack_dir` and runner adapters.
- **Path:** `src/runner.rs`  
  **Role:** Runner/deployer integration adapters and helpers.  
  **Key functionality:** In-memory provider factory for tests/CLI; runner-backed provider/bridge factories built from callables; WIT-friendly envelope structs and conversions; `RunnerBrokerProvider` that polls a runner client for events and auto-acks when configured; function-backed bridge helpers; `NoopProvider` for drop-only publish.  
  **Key dependencies / integration points:** Expects host-provided `RunnerBrokerClient`/`RunnerBridgeClient` implementations; uses `subscription_channel` to stream events.
- **Path:** `src/bridge.rs`  
  **Role:** Bridge traits and registry/service for message↔event conversion components.  
  **Key functionality:** Traits for bridges and bridge factory; registry to register/fetch bridges; service that handles invocation and maps errors to bus errors.
- **Path:** `src/acl.rs`  
  **Role:** Topic ACL evaluation.  
  **Key functionality:** `TopicAclRule` matching tenant/topic patterns; `TopicAcl` allowing/denying publish/subscribe with default allow behavior.
- **Path:** `src/backoff.rs`  
  **Role:** Retry/backoff policy definitions.  
  **Key functionality:** `BackoffStrategy` (fixed/exponential/jitter) and `RetryPolicy` with retryable error filtering and delay calculation; jitter helper using `rand`.
- **Path:** `src/subscription.rs`  
  **Role:** Subscription handle utilities.  
  **Key functionality:** `AckMode`, `SubscriptionOptions`, `SubscriptionHandle` implementing `Stream`, and `subscription_channel` helper for provider implementations.
- **Path:** `src/metrics.rs`  
  **Role:** Lightweight counters plus OpenTelemetry hooks for bus operations.  
  **Key functionality:** Atomic counters for events in/out/fail/dlq; OTEL histogram/counters for latency and counts; recording helpers used by the bus.
- **Path:** `src/idempotency.rs`  
  **Role:** Idempotency key storage abstraction.  
  **Key functionality:** `IdempotencyStore` trait and in-memory `InMemoryIdempotencyStore` using a mutexed `HashSet`.
- **Path:** `src/pattern.rs`  
  **Role:** Simple glob-style pattern matcher for tenant/topic matching.  
  **Key functionality:** Supports `*` and `?` wildcards; used by ACL and topic routing.
- **Path:** `src/bin/greentic-events.rs`  
  **Role:** CLI that loads providers from a pack using in-memory noop factories and prints provider descriptors.  
  **Key functionality:** Clap-based argument parsing for `--pack`; uses `EventBusBuilder::register_from_pack_dir` and lists provider metadata.
- **Path:** `src/main.rs`  
  **Role:** Minimal default binary entrypoint.  
  **Key functionality:** Prints “Hello, world!” (library functionality lives in `lib.rs` and the CLI binary).
- **Path:** `docs/runner-integration.md`  
  **Role:** Notes on wiring runner-hosted providers/bridges into the bus using the runner adapters.
- **Path:** `docs/secrets-events.md`  
  **Role:** Secrets topic conventions and example payloads (metadata-only, no secret values).
- **Path:** `MIGRATION_STATUS.md`  
  **Role:** Tracks secrets migration status for this repo (changes, breakages, downstream actions).
- **Path:** `tests/`  
  **Role:** Integration/unit coverage for bus behavior, runner/WIT adapters, and CLI smoke.  
  **Key functionality:** Exercises pack loading, retries/DLQ/idempotency, ACLs, metrics, spans, bridge adapters, runner clients, secrets topic payload acceptance, and CLI provider listing (fixtures under `tests/fixtures/packs`).

## 3. Work In Progress, TODOs, and Stubs
- None identified (no TODO/FIXME markers or unimplemented stubs found).

## 4. Broken, Failing, or Conflicting Areas
- `cargo test` (including integration and CLI smoke) passes locally; no failing tests or build errors observed.

## 5. Notes for Future Work
- Replace the default noop/closure-based factories with real runner/deployer client bindings in host applications, as described in `docs/runner-integration.md`.
- Consider removing or repurposing `src/main.rs` if a standalone binary beyond the CLI is needed.
