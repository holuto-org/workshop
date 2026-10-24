---
description:
  "Privacy and signal policy for tracing, error capture and Sentry lifecycle"
condition:
  - "tracing|sentry|before_send|before_breadcrumb|instrument|println!|eprintln!|log::"
scope: "tool:edit(crates/**), tool:write(crates/**)"
interruptMode: never
---

# Telemetry

[The telemetry policy](../../docs/telemetry.md) is the data allowlist and
explains native-dump privacy. Instrumentation in posnet/agent uses `tracing`,
not direct Sentry capture (agent API may read trace propagation headers).
Desktop owns the Sentry client, OS-root transport, subscriber, scrubbers,
sessions and native reporter.

- Instrument only IDs, command names, states, attempts and device error codes;
  the structured transport retains a narrower allowlist, not arbitrary fields.
  Never customer/company names, NIP, line text, amounts, token, full URL/path/query,
  hostname, local paths or raw request/response/problem bodies. Do not instrument
  arguments with automatic `Debug`; skip payloads/secrets. Route names are fixed
  templates, not actual IDs/URLs/cursors.
- ERROR: a person must act — uncertain fiscal outcome, corrupt journal, worker
  panic, unexpected API contract violation. WARN: handled degradation —
  not-ready device, operator wait, retrying start. INFO: milestones — connected,
  claimed, receipt/report printed. No span/INFO per normal three-second poll.
- Agent owns the single fiscal-operation span with
  `sentry.op = "fiscal.receipt"` or `"fiscal.report"`, job ID/attempt/kind;
  nested driver spans use `device.receipt`/`device.report`. One HTTP-call span
  with `sentry.op = "http.client"`, method/template/status. Forward only
  SDK-provided trace headers to Holuto: 0.49.3 emits `sentry-trace`, not
  `baggage`; do not fabricate baggage or log propagation contents.
- Device tags are Holuto printer ID and physical serial, not staff/customer
  identity. Keep default PII disabled and host/server identity removed. Scrub
  errors, breadcrumbs, logs **and transactions**; test the full envelope through
  a capturing transport with synthetic secret/PII canaries.
- No build-time DSN or saved opt-out means no client transmission at startup.
  Live opt-out gates new structured signals/envelopes; restart to disable the
  native reporter or initialize a client after opting in. Read the preference
  and initialize Sentry before the instance lock, fiscal journal or worker: the
  native reporter re-executes the binary and must enter initialization even if
  consent changed after its parent launched. Native memory is not protected by
  structured hooks; never promise it contains no sensitive data.

Changing capture fields, retention or opt-out behavior changes the documented
privacy contract: update policy and tests in the same change.
