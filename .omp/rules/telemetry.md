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
- When changing consent, gate structured capture and Holuto trace headers with
  current shared consent. Sampling zero or changing a thread-local Hub does not
  stop propagation on worker threads. Test off and back on with an active trace
  and loopback requests: headers follow consent; functional requests still work.
- Preserve synthetic secret/PII canaries even after their source UI is removed.
  Check serialized envelopes across errors/panics, breadcrumbs, logs **and
  transactions** with capturing transports, never live ingestion. `before_send`
  alone does not cover them all.
- Read the [native reporter exception and lifecycle](../../docs/telemetry.md#native-minidumps-important-exception)
  before moving startup or shutdown work. The reporter re-executes this binary.
  Secure state and crash directories before reporter initialization, without
  taking instance locks, touching the fiscal journal or starting workers. Fail
  initialization closed if preparation fails. Start the regression with no state
  directory; pre-opening it hides first-launch failures.
  Structured scrubbing does not sanitize dump memory; server-side deletion does
  not mean sensitive bytes were never transmitted.

When a change alters capture, retention or opt-out behavior, update the privacy
contract and its tests together. Do not weaken a privacy canary to accommodate
new instrumentation.
