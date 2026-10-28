---
description:
  "Privacy and signal policy for tracing, error capture and Sentry lifecycle"
condition:
  - ".*"
scope:
  - "tool:edit(crates/*/src/**)"
  - "tool:write(crates/*/src/**)"
  - "tool:edit(crates/desktop/ui/**)"
  - "tool:write(crates/desktop/ui/**)"
  - "tool:edit(Cargo.toml)"
  - "tool:write(Cargo.toml)"
  - "tool:edit(docs/telemetry.md)"
  - "tool:write(docs/telemetry.md)"
interruptMode: never
---

# Telemetry

Read [the privacy contract](../../docs/telemetry.md) before changing capture,
SDK features, transport or consent. It owns the allowlist, signal policy and
lifecycle; do not duplicate those inventories here.

- Keep sensitive data out of instrumentation at its source, not merely out of
  the final event. Skip payloads/secrets in `#[instrument]`; never dump settings,
  jobs, receipts or HTTP bodies through `Debug`, messages or fields. An ID is
  not automatically anonymous or allowed by the contract.
- Choose stable, actionable signals rather than successful-poll noise. Review
  both message text and fields: a new fixed message may need allowlisting, but
  a useful local field is not by itself a reason to export it.
- Preserve synthetic secret/PII canaries at the serialized-envelope boundary
  across errors/panics, breadcrumbs, logs **and transactions**. `before_send`
  alone does not cover them all; use capturing transports, never live ingestion.
- Read the [native reporter exception and lifecycle](../../docs/telemetry.md#native-minidumps-important-exception)
  before moving startup or shutdown work. The reporter re-executes this binary.
  Structured scrubbing does not sanitize dump memory; server-side deletion does
  not mean sensitive bytes were never transmitted.

When a change alters capture, retention or opt-out behavior, update the privacy
contract and its tests together. Do not weaken a privacy canary to accommodate
new instrumentation.
