# Diagnostics and privacy

Holuto Workshop uses Sentry project **`holuto/workshop`** in the EU
(`de.sentry.io`), provisioned by the project owner. This is separate from the
printer identity/VAT telemetry sent to Holuto's `/agent/session` and
`/agent/heartbeat` for fiscal processing.

## Enable and opt out

The public DSN and environment are baked into the binary from build-time
`SENTRY_DSN` and `SENTRY_ENVIRONMENT` (`option_env!`). Without a DSN, telemetry
is disabled; simulator builds never report either. Ordinary local/dev/test
builds do not report. `SENTRY_AUTH_TOKEN` is an upload credential used only by
CI, never an application setting or binary value.

The **Ustawienia → Udostępniaj diagnostykę** toggle defaults on; an explicit off
value survives restart. Turn it off and restart the app to ensure no diagnostics
client/native crash reporter remains active. This affects diagnostics, not the
Holuto requests required to print. Tests use a capturing transport, not the
production DSN.

## What is sent

Structured diagnostics use an allowlist:

- **Errors and Rust panics**: Actionable error classification, sanitized
  message and stack frames; SDK/platform/release/environment and timestamps;
  no raw fiscal payload or arbitrary error body.
- **Logs and breadcrumbs through `tracing`**: Operation milestones and
  handled degradation: IDs, command names, states, attempt numbers, device
  error codes; safe fixed text.
- **Fiscal traces**: One agent span per receipt/report (`fiscal.receipt` /
  `fiscal.report`), job ID/attempt/kind and timing/status; nested driver spans
  use `device.receipt` / `device.report`, not another fiscal operation.
- **HTTP traces**: Method, route **template** such as
  `/agent/jobs/{id}/result`, status and timing; forward the SDK's
  trace-propagation headers to `/agent` so backend traces can link. SDK 0.49.3
  currently emits `sentry-trace`, not `baggage`; future SDK-provided baggage
  is forwarded, never fabricated.
- **Release health**: Application-mode sessions: generated session identity,
  release/environment, start/duration and healthy/error/crash/exit status
  needed for crash-free-session accounting.
- **Device context**: Holuto printer ID and physical device serial as tags
  once known; these are device identifiers, not anonymous data.
- **Native crash**: Fatal event and native minidump, with matching
  release/debug identifiers and available allowlisted context; see the memory
  privacy warning below.

No span or INFO message is created for each normal three-second poll. ERROR
means someone must act (uncertain fiscal outcome, corrupt journal, worker panic,
unexpected API contract violation); WARN is degraded but handled (printer not
ready, operator wait, retrying start); INFO is a milestone (connected, claimed,
receipt/report printed).

The desktop owns Sentry initialization, transport using OS trust roots, tracing
subscriber, scrubbing, sessions and minidump reporter. Driver/agent
instrumentation uses `tracing`; the API only reads Sentry trace-propagation
headers. Structured event, breadcrumb, log and transaction surfaces must all be
scrubbed/allowlisted; `before_send` alone does not cover every signal
([Sentry sensitive-data guidance](https://docs.sentry.io/platforms/rust/data-management/sensitive-data/),
[Rust logs](https://docs.sentry.io/platforms/rust/guides/tracing/logs/)).

## What structured diagnostics never send

- Customer/company/person names, buyer NIP, receipt line names/text, amounts,
  prices, quantities or payment/deduction payloads.
- Pairing codes, the device token, authorization headers, passwords, arbitrary
  server response/problem bodies, settings or journal/history contents.
- Actual API URL path/query, opaque pagination cursor, hostname/server name,
  user-account identity, home directory or local state path. HTTP spans carry
  fixed route templates instead.
- Screenshots, session replay, receipt/document attachments or automatic copies
  of local files.
- Stack-frame local paths are removed; file basenames and debug identifiers
  remain for symbolication.

Keep `send_default_pii` disabled; remove host/server identity rather than
assuming the SDK default is safe. Do not log `Debug` of settings, jobs,
receipts, requests or responses. Synthetic canaries and a capturing transport
prove the policy across errors, logs, breadcrumbs and traces; see
[security tests](testing/security.md). Network services still see connection
metadata (for example source IP at ingestion); the application does not attach a
user-IP field.

## Native minidumps: important exception

**A native dump is process memory, not a scrubbed structured event.** It can
contain environment values, local paths, input fields, the token or
customer/receipt data that happened to be in memory. Local structured-event
hooks cannot promise that those bytes never leave the computer. Opt out if this
risk is unacceptable.

[Sentry's Rust native-crash documentation](https://docs.sentry.io/platforms/rust/native-crash-reporting.md)
states that dumps are processed and immediately discarded by default, with
sensitive information stripped from resulting issues; project administrators can
instead enable storage as attachments. Keep attachment retention disabled. This
server-side handling is not a guarantee that no sensitive bytes were
transmitted.

The minidump reporter re-executes the same binary. Read the telemetry preference
without acquiring the instance lock; initialize Sentry before taking that lock,
touching the fiscal journal or starting worker threads. Otherwise the reporter
can collide with the application or mutate fiscal state. Only allowlisted device
context may be forwarded to the crash reporter; it does not automatically
inherit later scope changes
([reporter lifecycle](https://docs.sentry.io/platforms/rust/native-crash-reporting.md#how-it-works)).

## Release and debug files

Release identity is **`workshop@<workspace version>`**, environment
**`production`**. CI builds all five native targets with debug information,
retains matching symbols before distribution stripping, and uploads debug
files/source context using `sentry-cli debug-files upload --include-sources`.
Source uploads contain application source code, not runtime settings or fiscal
documents. Release records/commits and client events must agree on identity;
Linux ELF/DWARF, macOS dSYM and Windows PDB files must match their packaged
binary.

See [CI configuration](ci.md#release-telemetry-artifacts) for variables/secrets
and [release verification](workflows.md#sentry-release-check). Uploads enable
readable frames, not automatic proof that symbolication works;
[Sentry source-context documentation](https://docs.sentry.io/platforms/rust/source-context/)
explains the artifacts.
