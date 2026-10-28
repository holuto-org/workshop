# Diagnostics and privacy

[CI configuration](ci.md#release-telemetry-artifacts) owns Sentry provisioning
and repository variables. Diagnostics are separate from printer identity and VAT
telemetry sent to Holuto's `/agent/session`; `/agent/heartbeat` carries the
device identity header, not that telemetry body.

## Enable and opt out

The public DSN and environment are baked into the binary from build-time
`SENTRY_DSN` and `SENTRY_ENVIRONMENT` (`option_env!`). Without a DSN, telemetry
is disabled; simulator builds never report either. A debug build with a DSN
can report, using environment `development`. `SENTRY_AUTH_TOKEN` is an upload
credential used only by CI, never an application setting or binary value.

The **Udostępniaj diagnostykę** settings toggle defaults on; a successfully saved
off value survives restart. Missing settings use that default; unreadable or
malformed settings disable diagnostics at normal startup. Turning the toggle
off immediately gates new structured capture, envelopes and Holuto trace
propagation. Queued envelopes are checked again on the HTTP thread; an HTTP
request already started cannot be recalled. Restart to disable the native crash
reporter. Turning it on after a disabled startup also needs a restart to
initialize the client. Diagnostics consent does not disable Holuto requests
required to print.
To opt out before the first GUI launch, run
`workshop config set --diagnostics off` while the application is stopped.
Normal CLI commands do not initialize diagnostics. Package smoke isolation
follows the [package checks](testing/platforms.md#package-checks).

A native reporter child bypasses CLI dispatch and enters initialization even if
consent changed after its parent launched. Its separate process does not receive
the parent's live consent flag; this is why native opt-out needs restart.

## What is sent

Structured diagnostics use an allowlist:

- **Errors and Rust panics**: Actionable error classification, sanitized
  message and stack frames; SDK/platform/release/environment and timestamps;
  no raw fiscal payload or arbitrary error body.
- **Logs and breadcrumbs through `tracing`**: WARN/INFO become logs and
  breadcrumbs; ERROR becomes an error event; DEBUG/TRACE are ignored. Only
  reviewed fixed messages survive; other text becomes
  `Fiscal application diagnostic`. Arbitrary fields, including attempts and
  device error codes, are removed.
- **Fiscal traces**: Only `fiscal.receipt` / `fiscal.report` roots are sampled,
  at rate 1.0 while diagnostics are on; every other root at 0. Nested spans use
  `device.receipt` / `device.report` and `http.client`. Transactions are named
  `fiscal.operation`; timings/status and span operations remain, but
  descriptions, span data/tags and transaction extras are cleared.
- **HTTP trace propagation**: While diagnostics are on, SDK-provided headers
  link requests to Holuto, including unsampled requests. The API records
  method/route-template/status locally; structured transport removes their
  span data. SDK 0.49.3 emits `sentry-trace`, not `baggage`; future SDK-provided
  baggage is forwarded, never fabricated.
- **Release health**: Application-mode sessions: a generated session identity,
  release/environment, start/duration, error count and session status needed for
  crash-free-session accounting. Workshop does not set a user identity.
- **Device context**: Holuto printer ID and physical device serial as tags
  once connected, cleared when processing stops; these are device identifiers,
  not anonymous data.
- **Native crash**: Fatal event and native minidump, with matching
  release/debug identifiers and available allowlisted context; see the memory
  privacy warning below.

Normal three-second polling creates HTTP-call spans but no poll-specific root
span or INFO message. ERROR
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

- Customer/company/person names, buyer NIP,
  receipt line names/text, amounts, prices, quantities or payment/deduction
  payloads.
- Pairing codes, the device token, authorization headers, passwords, arbitrary
  server response/problem bodies, settings or journal contents.
- Actual API URL path/query, hostname/server name,
  user-account identity, home directory or local state path. HTTP spans carry
  fixed route templates instead.
- Screenshots, session replay, receipt/document attachments or automatic copies
  of local files.
- Stack-frame local paths are removed; file basenames and debug identifiers
  remain for symbolication.

Keep `send_default_pii` disabled; remove host/server identity rather than
assuming the SDK default is safe. Do not log `Debug` of settings, jobs,
receipts, requests or responses. Synthetic canaries and capturing transports
check serialized outgoing errors/panics, logs, breadcrumbs and transactions;
see [security tests](testing/security.md). These tests do not demonstrate
server retention, native-dump scrubbing or delivery. Network services still see
connection metadata (for example source IP at ingestion); the application does
not attach a user-IP field.

## Native minidumps: important exception

**A native dump is process memory, not a scrubbed structured event.** It can
contain environment values, local paths, input fields, the token or
customer/receipt data that happened to be in memory. Local structured-event
hooks cannot promise that those bytes never leave the computer. Opt out if this
risk is unacceptable.

The reporter writes the dump to `crashes/` in the private state directory.
Before starting it, Workshop creates or secures both directories without taking
state locks; failure disables diagnostics initialization. On Unix both directories
are **0700**. The SDK-created dump file itself follows the process umask; its
privacy depends on those protected directories, not a promised **0600** file mode.
The SDK's default shared temporary `Crashes` directory would not protect it.

The [SDK dump writer][dump-writer] attempts deletion after its upload callback,
whether delivery succeeds or not. Interrupted or failed capture, upload handling
or deletion can leave a sensitive file there; there is no later-startup upload
or cleanup scan.

[Sentry's Rust native-crash documentation](https://docs.sentry.io/platforms/rust/native-crash-reporting.md)
states that dumps are processed and immediately discarded by default, with
sensitive information stripped from resulting issues; project administrators can
instead enable storage as attachments. Keep attachment retention disabled. This
server-side handling is not a guarantee that no sensitive bytes were
transmitted.

The minidump reporter re-executes the same binary. Normal application startup
reads the preference without an instance lock, then initializes Sentry before
taking that lock, touching the fiscal journal or starting worker threads.
Only allowlisted printer tags are explicitly forwarded to the reporter;
later scope changes are not automatically inherited
([reporter lifecycle](https://docs.sentry.io/platforms/rust/native-crash-reporting.md#how-it-works)).

Client shutdown and native-reporter flushing use a **500 ms** timeout.
The bounded best-effort transport keeps SDK HTTP ownership and disposal off the
application thread; it may drop diagnostics rather than delay fiscal shutdown.
Its HTTP requests have a separate three-second timeout, use OS trust roots,
refuse redirects and follow the environment proxy. The Holuto client bypasses
proxies for loopback.

Structured envelopes are queued in memory, not persisted for later delivery.
The client does not set a server retention period for events, logs, traces or
sessions; administrators must verify project/service retention separately.
Keeping native dump attachment storage disabled is a separate requirement, not
a retention limit for those structured signals.

The Linux native-crash regression uses a crashing child and a capturing
transport to check first-launch dump placement and private Unix directories.
It does not prove remote delivery, symbolication, server retention or
Windows/macOS native-crash behavior.

## Release and debug files

The [CI release identity and environment](ci.md#release-telemetry-artifacts)
must match client events. Non-CI release builds use `SENTRY_ENVIRONMENT`,
defaulting to `production` only when unset.
Matching Linux ELF/DWARF, macOS dSYM and Windows PDB files are needed for the
packaged binary. The executable itself also carries unwind information needed
for native stack walking. Source-context uploads contain application source
code, not runtime settings or fiscal documents.

[CI configuration](ci.md#release-telemetry-artifacts) owns upload mechanics,
identity and credentials; [release verification](workflows.md#sentry-release-check)
owns the operator check. Uploads enable readable frames, not automatic proof
that symbolication works;
[Sentry source-context documentation](https://docs.sentry.io/platforms/rust/source-context/)
explains the artifacts.

[dump-writer]: https://docs.rs/minidumper-child/0.5.0/src/minidumper_child/server.rs.html
