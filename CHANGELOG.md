# Changelog

All notable changes to this project will be documented in this file.

## Unreleased
- (none)

## 0.4.3
- Add generic helper API (`EventBus::publish_event`, `subscribe_topic`, and `build_envelope`) for string-based topics with JSON payloads; no breaking changes (patch-level).
- Document recommended greentic-repo topic conventions while keeping events untyped and domain-agnostic, with publish/subscribe snippet.
- Extend tests to cover the helper APIs and generic topic handling.
- Inline pack loading with basic packVersion/id/version validation while still validating events/annotations.
