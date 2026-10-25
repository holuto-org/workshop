# Security and privacy tests

## Pairing, token and transport

The device token is a secret, not a staff login. Use conspicuous synthetic
canaries in settings, malformed server errors and test fixtures. Assert absence
from `Debug`, operator errors, panic/log output, breadcrumbs, event extras,
traces and serialized Sentry envelopes. A test checking only one successful
request misses error paths.

The production client must prove:

- HTTPS except genuine loopback (`localhost`, `127.0.0.0/8`, `::1`), without URL
  credentials, query or fragment. Test deceptive hostnames and both boundaries;
  retain a permitted path prefix.
- OS trust roots and certificate verification; an untrusted local TLS endpoint
  must fail. Never disable TLS verification for the fake.
- Redirects are refused without forwarding bearer or device headers to their
  target. Assert zero target requests.
- Proxy handling is explicit and tested. In a child process, give hostile
  `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY` and `NO_PROXY` values; assert the
  documented destination and no credential escape. Never change process-global
  environment between parallel tests.
- Exact bearer, serial, version and Polish-language headers; 401 always stops
  without refresh. Malformed, oversized and unexpected bodies fail with the
  correct error and no print.

Use loopback fakes for failures; never production Holuto tokens or live customer
fixtures.

Pairing codes are secrets too. Through the production HTTP client prove code
normalization, single-use/expiry/refusal/throttling behavior and that a failed
exchange does not replace saved credentials. A token is bound to the API address
it was paired with: changing the URL (CLI or GUI) forgets it, while the same
address in another spelling keeps it. CLI `config show` redacts credentials;
every mutating CLI command must refuse the GUI's instance lock.

## Local state

On Unix assert state directory 0700 and created/replaced data files 0600,
including settings, status, journal and history. Opening the state tightens an
existing directory/lock; loading history tightens its file. Settings/journal/
status reads alone do not repair modes. On Windows use the Local AppData Known
Folder and profile ACL protection; Unix modes do not prove Windows privacy.
Test cross-process locking and atomic replacement on every supported OS.

Inject a deterministic write failure (for example a directory occupying the
target file), not chmod assumptions that disappear when CI runs as root. Assert
old journal evidence survives failure. Never use permission failure as a reason
to delete/reset the journal.

## Structured telemetry

[Telemetry policy](../telemetry.md) is the allowlist. Capture through a test
Sentry transport, not a live DSN. Exercise error, panic, breadcrumb, log and
transaction paths; a scrubbed error alone does not prove logs or transactions
are safe. Seed customer name, NIP, line name, amount, token, full API URL and
hostname canaries and inspect entire envelopes, including nested fields and
breadcrumbs.

Assert printer ID/device serial tags only once known, safe route templates
rather than real URL paths, unchanged trace propagation semantics, and no
transmission when DSN is absent or opt-out is saved. No test may send real crash
reports.

Sentry's
[`before_send` guidance](https://docs.sentry.io/platforms/rust/data-management/sensitive-data/)
explains local event scrubbing; logs and transactions have separate surfaces and
must be checked separately. Keep `send_default_pii` off, remove server/host
identity and do not attach state files or request/response bodies.

## Native dumps are different

Native minidumps contain process memory and may include sensitive data even when
structured fields are clean. The Rust event scrubber cannot prove those bytes
private.
[Sentry's native-crash documentation](https://docs.sentry.io/platforms/rust/native-crash-reporting.md)
says dumps are processed and removed by default unless attachment retention is
enabled. Test crash capture only in an isolated child with synthetic data and a
capturing destination; verify opt-out disables it. Do not advertise an absolute
no-PII guarantee for dump memory.
