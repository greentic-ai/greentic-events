# Provider packs & workflows

## A) What is greentic-events?

`greentic-events` is the multi-tenant event fabric shared across Greentic. It wires the core `EventEnvelope` domain types into a provider/bridge registry that tracks retry policies, dead-letter queues, ACLs, and observability before wiring the resulting bus into your runner or CLI. Providers live outside the core crate so each host can ship its own event sources and transports: a **provider pack** binds a pack-level manifest (`pack.yaml`), flows/components, and configuration annotations that describe the event source (webhook listener, polling loop, etc.) plus how to normalize every incoming payload into `greentic-events` types.

## B) Provider model

### Provider packs

A provider pack (`*.gtpack` or its unpacked directory) packages:

1. Manifest (`pack.yaml`) that lists providers, entry flows, and metadata.
2. Component flows that handle event ingestion/normalization.
3. Configuration snippets or extensions that describe transports, retries, and ACLs.

Each pack exports one or more `events.providers.<name>` descriptors that `greentic-events` registers at runtime.

### Discovery

`greentic-events` can discover packs from:

| Source | How to point to it |
| --- | --- |
| **Local path** | `--pack ./providers/local/<PROVIDER>` or `--pack` plus `greentic.yaml` pointing at a repo checkout |
| **OCI / GHCR** | `./scripts/events_provider_fetch.sh --provider <PROVIDER> --pack-root ./providers/cache --oci-ref ghcr.io/greentic-ai/<PROVIDER>:latest` to fetch, then point `--pack` at the cached directory |

### Selection precedence

When multiple providers exist, Greentic resolves them in this order:

1. **Explicit CLI flag** (`--pack`, `--provider`, or any host-specific override passed at startup). That invocation wins everything else.
2. **Project config** (`greentic.yaml` under the project root) that enables/disables named providers.
3. **Environment variables** (for example, `GREENTIC_EVENTS_PROVIDER` or other runner-specific overrides that point to a provider pack).
4. **Default provider** baked into the runner (commonly `ghcr.io/greentic-ai/greentic-events-providers:latest`).

The CLI or runner can disable a provider by setting `enabled: false` in `greentic.yaml` even when a pack is present on disk.

## C) Quickstart: run greentic-events with a provider

### 1. Local provider pack

#### Layout
```
providers/
  local/
    ${PROVIDER}/
      pack.yaml
      dist/
      flows/

state/
  ${TENANT}/
    ${TEAM}/
      events.log
```

1. Clone or build your pack under `providers/local/${PROVIDER}`.
2. Give it state directories under `./state/${TENANT}/${TEAM}` so the runner can persist offsets.
3. Run the CLI:
```bash
export RUST_LOG=info
cargo run --bin greentic-events-cli -- --pack "./providers/local/${PROVIDER}"
```
4. Pass minimal config via `greentic.yaml` (see section below) or `--config ./greentic.yaml` to select the right provider.
5. Watch logs that show provider registration (`providers:` list printed on success) and any event handling. Tail `./state/${TENANT}/${TEAM}/events.log` or your host’s log sink to verify liveness.

### 2. GHCR provider pack

1. Fetch the published pack:
```bash
./scripts/events_provider_fetch.sh \
  --provider greentic-events-providers \
  --pack-root ./providers/cache \
  --oci-ref ghcr.io/greentic-ai/greentic-events-providers:latest
```
2. Run with the cached pack:
```bash
cargo run --bin greentic-events-cli -- --pack "./providers/cache/greentic-events-providers"
```
3. Use `greentic.yaml` or environment overrides to point your hosted runner at `providers/cache/greentic-events-providers`.
4. Verify the CLI prints every provider it registered and inspect `./state/${TENANT}/${TEAM}` or `./tests/project/state` for stateful evidence.

## D) Configuration

`greentic.yaml` is the canonical override that hosts usually drop beside their source tree. A minimal structure:

```yaml
events:
  providers:
    - name: ${PROVIDER}
      pack: ./providers/local/${PROVIDER}
      enabled: true
      mode: webhook
      mock: false
    - name: ${PROVIDER}-mock
      pack: oci://ghcr.io/greentic-ai/${PROVIDER}:latest
      enabled: true
      mode: poll
      mock: true
```

- `name` matches the identifier exported by the pack.
- `pack` is either a local directory or an OCI reference (allowing GHCR packs without caching).
- `enabled` lets you toggle providers without removing pack files.
- `mode` documents whether the pack expects `webhook` listeners or `poll` loops (optional but helpful for readers).
- `mock` toggles a test-friendly execution path so smoke tests can run without hitting live APIs.

Multiple entries can coexist; the CLI will register them all but obey the precedence rules above.

## E) Testing

### 1. Doctor checks

Validate every pack before you ship it:

```bash
greentic-pack doctor ./providers/local/${PROVIDER}
```

or, to iterate all known packs:

```bash
./scripts/events_provider_doctor.sh
```

Add `--provider ${PROVIDER}` to focus on a single bundle. Pass `--strict` to treat warnings as failures if your `greentic-pack` binary supports it.

### 2. Smoke tests

Smoke targets exercise both the pack metadata and the CLI without external dependencies:

```bash
./scripts/events_provider_smoke.sh --pack-root ./tests/fixtures/packs
```

That runs `greentic-events-cli` with every `pack.yaml` under `tests/fixtures/packs` or `./providers`. Expect exit code `0` and a short provider list printed to stdout. Use `mock: true` flows to emit canned events and inspect `./state/${TENANT}/${TEAM}` (or `./tests/project/state`) for files created during the run.

### 3. Integration tests

- Build e2e fixtures before running guard rails:
  ```bash
  cargo run --quiet --bin greentic-events-fixture -- --out tests/fixtures/packs
  ```
- Run the shared harness:
  ```bash
  cargo run --quiet --bin greentic-events-test -- e2e --packs tests/fixtures/packs --dry-run
  ```
- Add a new provider pack test by copying `tests/fixtures/packs/events_fake` into its own directory, wiring the flows in `pack.yaml`, and referencing it from `tests/fixtures/packs` before rerunning the harness.
- Always run `cargo test` (it covers the CLI smoke guard, config precedence checks, and unit coverage).

## F) CI gating recommendation

Gate every change that touches packs or provider selection with:

1. `cargo fmt && cargo clippy --workspace --all-targets --all-features`.
2. `cargo test --workspace --all-features`.
3. `./scripts/events_provider_doctor.sh` to lint packs from `./providers`, `./packs`, and `tests/fixtures/packs`.
4. `./scripts/events_provider_smoke.sh` to confirm CLI compatibility.

`ci/local_check.sh` already bundles these steps, so wire your CI job (see `.github/workflows/ci.yml`) to run that script with `CLI_SMOKE=1`. Publish only when all of the above succeed.
