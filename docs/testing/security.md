# Security and privacy tests

This page helps you choose evidence; it does not claim every check has run.
Linked tests provide regression coverage. Report native OS and release results
separately.

Use fresh temporary state, synthetic pairing codes/tokens/customer data,
`agent::testing::Holuto` on loopback and capturing Sentry transports. Printer
effects are tested only through the brand simulators. Never inspect real
settings, use customer fixtures, contact production Holuto or send crash reports.
For packaged applications, follow [package isolation](platforms.md#package-checks)
before any launch; a simulator feature is not a substitute for isolated state.

## Pairing, token and transport

The device token is a secret, not a staff login; pairing codes are secrets too.
Seed conspicuous canaries in valid and malformed inputs. Check operator errors,
`Debug`, retained error sources and serialized outgoing telemetry, not just one
successful response. Explicit token serialization into private settings is
intentional; redacted formatting does not encrypt the stored credential.

Required coverage through the production HTTP client:

- Reject remote HTTP URLs and any API URL containing credentials, queries or
  fragments before a request. Exercise deceptive loopback names, IPv4 boundaries
  and IPv6, and preserve allowed path prefixes.
- Reject an untrusted loopback TLS peer before it receives HTTP credentials;
  never disable certificate verification to make the fake work.
- Refuse redirects and assert **zero requests at the target**, not merely a
  returned error.
- Check proxy routing in a child process with hostile upper/lower-case proxy
  variables and `NO_PROXY`. Loopback must bypass environment proxies; remote
  HTTPS uses the client's environment proxy policy. A loopback bypass test does
  not prove remote proxy or OS trust-store behavior.
- Check bearer/version/language headers and device binding. Session sends the
  serial in its body, not the serial header used by other authenticated routes;
  pairing sends neither token nor printer identity.
- Check 401 without refresh/retry, malformed responses and exact body-size
  boundaries. API rejection alone does not prove no print: use an agent/worker
  scenario with simulator effects when the claim includes fiscal dispatch.

Start with [API tests](../../crates/agent/src/api/tests.rs): these include
redirect-target silence, hostile-proxy isolation, untrusted TLS, header binding,
safe errors and response limits. URL and credential persistence cases live in
[settings tests](../../crates/agent/src/settings/tests.rs).

Pairing evidence must include normalization, local refusal of malformed codes,
safe handling of server refusals and failure to save credentials.
[Worker pairing tests](../../crates/agent/src/worker/tests.rs) cover saving
before success notification, failure preservation and no printer opening;
[CLI tests](../../crates/desktop/src/cli/tests.rs) cover redacted output and all
mutations refusing an application or CLI instance lock. Changing the configured
API address must forget its token; equivalent normalized addresses retain it.
Do not share `config show` output blindly: it omits the token but includes the
configured API address and port.

[Fake-server tests](../../crates/agent/src/testing/tests.rs) exercise single-use
codes, credential revocation and scripted throttling. These model server policy,
not the deployed backend. A scripted invalid/expired-code response proves client
handling, not actual expiry or rate-limit timing.

## Local state

Use [store tests](../../crates/agent/src/store/tests.rs) for cross-process
exclusion, release after process exit, non-creating status reads and complete
replacement while an old reader stays open. Run OS-specific cases natively;
cross-compilation cannot prove filesystem or lock behavior.

Unix coverage includes tightening existing state/locks to 0700/0600 and
creating/replacing settings, journal and status with 0600. Their module tests
also show reads do not repair file modes. Windows chooses the Local AppData
Known Folder rather than an environment override; the implementation inherits
profile ACLs, it does not install a custom ACL. Known Folder/path tests and Unix
modes do not prove that another Windows account cannot read the files. Any such
privacy claim needs an isolated native multi-account check.

Inject deterministic write failure, not chmod assumptions that disappear under
root. The [journal regression](../../crates/agent/src/journal/tests.rs) uses its
write-failure hook and checks exact previous evidence across reopening;
[status tests](../../crates/agent/src/status/tests.rs) refuse replacement of a
nonempty destination directory. Preserve the previous record and assert no
fiscal send when persistence is a dispatch prerequisite. Never delete/reset
evidence to recover from a permission or write failure.

## Structured telemetry

[Telemetry policy](../telemetry.md) owns the allowlist and consent semantics.
Use a capturing transport with a synthetic DSN, never real ingestion. Exercise
errors, panics, breadcrumbs, logs, transactions and sessions; `before_send`
alone does not cover them all
([Sentry scrubbing guidance](https://docs.sentry.io/platforms/rust/data-management/sensitive-data/)).

Seed token, pairing code, customer/NIP/line/amount, URL/hostname and local-path
canaries in nested fields and malformed errors. Inspect the entire serialized
envelope. Also assert the expected event/log/transaction actually exists:
dropping every signal would pass an absence-only test.

[Telemetry tests](../../crates/desktop/src/telemetry/tests.rs) check structured
capture, opt-out gating (including sessions), absent DSN and simulator gating.
Capturing transports do not prove delivery. For live opt-out, distinguish
capture-time gating from the HTTP-thread consent check: an HTTP request already
started cannot be recalled.
[Path tests](../../crates/desktop/src/telemetry/privacy/tests.rs) check that
symbols/debug identifiers survive without local paths or source text. Retain
useful classifications and known device tags, and clear stale printer identity
when processing stops. API tests check local route templates and exact SDK
trace-header propagation, not permission to transmit raw URL paths.

## Native dumps are different

Native minidumps contain process memory, outside the structured-event scrubber.
The [native privacy policy](../telemetry.md#native-minidumps-important-exception)
owns this exception, private dump storage and retention guidance.
[Sentry's native-crash documentation](https://docs.sentry.io/platforms/rust/native-crash-reporting.md)
describes server processing, not proof that sensitive bytes never leave the PC.

The Linux `native_crash_dump_is_written_inside_the_private_state_directory`
test in [telemetry tests](../../crates/desktop/src/telemetry/tests.rs) crashes an
isolated child with a capturing transport, including in the re-executed reporter.
It checks the dump location, 0700 state/crash directories on first launch and no
fiscal locks or journal creation. It does **not** prove upload, symbolication,
Windows/macOS dump handling or absence of private memory. Reporter-marker
detection is not an end-to-end delivery test.

Any additional native-crash test needs synthetic process data, private temporary
state and a capturing destination in the re-executed reporter too. Test startup
with saved opt-out separately from changing consent at runtime: structured
signals stop immediately, but disabling an already started native reporter
requires restart. Never claim an absolute no-PII guarantee for dump memory.

## Installer trust boundary

Use [installer fixtures](../../scripts/test-install.sh) and their
[PowerShell counterpart](../../scripts/test-install.ps1) for malformed, missing
or duplicate checksum entries, attestation failure, refused replacement and
preservation of existing files. Failed verification must not reach installation.
These tests fake downloads and external effects; they do not authenticate a real
release or prove SmartScreen/Gatekeeper reputation.

Record which fixture branches ran, not just the final success message. Shell
full-install cases skip under root; Linux package cases need Ubuntu/Debian, and
macOS app replacement needs PlistBuddy. PowerShell tests outside Windows replace
Internet-zone writes and skip native stream/registry checks.

Test checksum integrity separately from provenance. Both installers warn and
allow checksum-only verification when `gh` is absent; a present verifier's failure
is fatal. Real attestation checks need `gh` 2.68.0 or newer, signed in to
github.com. Preserve workflow/branch-bound verification arguments, macOS
quarantine and Windows Internet-zone marking.

The [install guide](../install-with-ai.md#2-pin-download-and-verify-before-running-anything)
owns user trust and consent; [package checks](platforms.md#package-checks) own
native isolation and smoke evidence. Neither installation nor pairing authorizes
processing, printing or journal resolution.
