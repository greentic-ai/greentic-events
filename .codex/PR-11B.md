# PR-11B.md (greentic-events)
# Title: Provide pack/flow templates for provider-core events integration

## Goal
Add reusable flow/node templates demonstrating how to publish events via provider-core
WITHOUT hardcoding any specific provider.

## Deliverables
1) Example flow templates:
- `flows/events/publish_event.flow.(json|yaml|gtc)` (use your flow format)
  - uses ProviderInvoke node kind (runner PR-07/08)
  - op: "publish"
  - maps payload/state into publish input JSON
  - writes publish output (receipt_id etc.) to state/payload

2) Docs:
- `docs/events_provider_core_flows.md`:
  - selecting provider_id/provider_type at runtime
  - mapping input/output
  - suggested topic naming conventions

## Acceptance criteria
- Any future event provider can be swapped in by changing provider_id/provider_type only.
