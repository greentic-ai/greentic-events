# GE-PR-02 Migration: Provisioned event installs

Event runtime now consumes provider install records produced by `greentic-provision`.
If you currently inject config/secrets via environment variables or ad-hoc config files, migrate to installs.

## What changes
- Runtime looks up provider installations and resolves config/secrets from install records.
- Multiple installs per provider are supported (for example multiple GitHub orgs).
- Subscription/webhook state is persisted in the install record over time.

## How to migrate
1. Re-run provisioning for each events provider (source/sink) to generate install records.
2. Store install records in your install store (or existing provisioning backend).
3. Ensure config/secret references resolve in your config/secret stores.
4. Update runtime wiring to select `provider_id` + `install_id` instead of env vars.

## Verification
- Start the runtime with only install records available.
- Emit an event to a specific install and confirm delivery.
- Run subscription sync and verify install record metadata updates.
