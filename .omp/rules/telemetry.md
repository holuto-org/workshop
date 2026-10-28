---
description:
  "Privacy and signal policy for tracing, error capture and Sentry lifecycle"
condition:
  - "tracing|sentry|before_send|before_breadcrumb|instrument|log::"
scope: "tool:edit(crates/*/src/**), tool:write(crates/*/src/**)"
interruptMode: never
---

# Telemetry

Read [the telemetry policy](../../docs/telemetry.md) before changing capture.
It owns the data allowlist, severity levels, sampling, propagation and consent
lifecycle. Instrument driver/agent code through `tracing`; keep Sentry client
ownership in desktop.

- Instrument only IDs, command names, states, attempts and device error codes;
  the structured transport retains a narrower allowlist, not arbitrary fields.
  Never customer/company names, NIP, line text, amounts, token, full URL/path/query,
  hostname, local paths or raw request/response/problem bodies. Do not instrument
  arguments with automatic `Debug`; skip payloads/secrets. Route names are fixed
  templates, not actual IDs/URLs.
- Follow the [signal policy](../../docs/telemetry.md#what-is-sent); do not add
  poll-specific root spans or INFO messages. Keep normal HTTP-call spans.
- Agent owns the single fiscal-operation span with
  `sentry.op = "fiscal.receipt"` or `"fiscal.report"`; receipt roots include
  job ID/attempt/kind. Nested driver spans use `device.receipt`/`device.report`.
  One HTTP-call span
  with `sentry.op = "http.client"`, method/template/status. Forward only
  SDK-provided trace headers to Holuto; never fabricate or log their contents.
- Test errors, breadcrumbs, logs **and transactions** through a capturing
  transport with synthetic secret/PII canaries, not just `before_send`.
- Preserve the [native reporter lifecycle](../../docs/telemetry.md#native-minidumps-important-exception):
  initialize before the instance lock, fiscal journal or worker; enter
  initialization in reporter children even after consent changes.

Changing capture fields, retention or opt-out behavior changes the documented
privacy contract: update policy and tests in the same change.
