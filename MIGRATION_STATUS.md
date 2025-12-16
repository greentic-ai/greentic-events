# Secrets Migration Status — greentic-events

- What changed: Added secrets event topic conventions doc (`docs/secrets-events.md`) with example payloads and redaction rules; added tests to ensure secrets topics and metadata payloads are accepted unchanged.
- What broke: Nothing observed; `ci/local_check.sh` passes.
- Repos to update next: None for greentic-events; downstream repos should consume the documented topic conventions as needed.
