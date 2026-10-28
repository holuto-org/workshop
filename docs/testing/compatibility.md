# Compatibility testing

## Stored-file contracts

The repository pins the on-disk formats. Keep sanitized, literal golden
fixtures, not fixtures serialized by the reader/writer under test. Never infer
that an unreadable journal can be discarded.

- **`operation.json`**: Receipt/report variants include vendor and unique number.
  `NotStarted`, `Dispatching`, completed with/without number, results and
  settlements retain their meaning across restart. Resolution requires the same
  vendor and unique number and never reissues commands.
- **`settings.json`**: API URL/token/port/baud/diagnostics/autostart and nullable
  vendor retain meaning; unknown fields and a missing `vendor` fail closed.
  Diagnostics opt-out survives save/load/restart.

A round trip proves only today's writer agrees with today's reader; literal
fixtures make future format changes deliberate. Until the first Workshop
release a format change is a clean cutover that updates the fixtures (see
[external surfaces](../../AGENTS.md#external-surfaces)). Never treat an
unreadable journal as permission to print. File replacement/locking behavior
must pass on each OS; Linux rename does not prove Windows replacement.

## `/agent` and the backend

The vendored OpenAPI is the production contract, not the fake. Compare session
vendor + unique number binding, heartbeat identity headers, current per-driver
limits/VAT/capabilities, document and job-history pagination/search, queue
inputs, take attempts, terminal states and result idempotency/refusals.
Update the `agent::testing` loopback fake in the same change, then prove
requests through
production `ureq` and the observable stored server state.

Follow the [sync procedure](../../AGENTS.md#api-contract).

Full OpenAPI schema validation belongs to the feature-gated testing fake, not
the runtime HTTP client. Fiscal amount/name/NIP scalars deliberately remain
strings until domain validation; a malformed individual fiscal job must fail at
the fiscal boundary rather than making an entire batch unreadable. Wire types
do not replace those domain checks.

Assert required nullable fields, decimal strings, unchanged opaque cursor
encoding, Polish problem details and exact result echo. Include commit-then-drop
and refusal for a stale attempt. Do not invent staff resolution for
failed/expired jobs: only unknown jobs have that backend resolution path. A
completed print conflicting with failed/expired uses explicit local
acknowledgement and no server write.

Job-history requests are read-only, never heartbeats. Assert the job's required
nullable `issuedAt` (absent from the history row), all lifecycle states and
staff/device sources, empty pages and both cursor directions. Changed
filter/search/sort with a token must fail at `/cursor/cursor`; blank/overlong
queries fail at `/search/q`. Search fixtures cover customer, plate, receipt
number, order number and line text without putting their values into logs or
`Debug`.

A fake alone cannot prove compatibility with a deployed server. Review backend
contract changes together; use sanitized recorded contract examples and the
backend's own tests, never probe production with a real token or queue fiscal
work as a compatibility check.

Sessions require `vendor`, `uniqueNumber`, `capabilities.maxQuantityDecimals`,
`capabilities.advanceDeductions` and `capabilities.zeroTotalReceipts`. Test
backend refusal of unsupported advances, zero-total sales and excess quantity
precision, plus independent local refusal before any fiscal command.
Company-footer availability remains a local header check.

## Dependencies and features

Keep Cargo.lock and workspace versions deliberate. `slint` and
`i-slint-backend-testing` move in lockstep; the testing backend is pinned
exactly (`=1.18.1` for Slint 1.18.1). It is an internal preliminary API without
patch-level semver guarantees
([upstream warning](https://docs.rs/i-slint-backend-testing/1.18.1/i_slint_backend_testing/)).
Compile and run accessible-label scenarios on upgrades; do not patch around a
mismatched backend.

Exercise default production features as well as all debug/test features. Desktop
release builds reject `simulator`; agent builds without debug assertions reject
`testing`. The drivers' own `simulator` features have no release guard. A green
all-features test run does not prove the real serial branch builds. Rebuild and
verify all five OS/architecture packages for dependency/toolchain upgrades; run
advisory/license and unused-dependency checks. See [CI](../ci.md).
`--all-features` must retain production fiscal-mode and unqualified-capability
gates. Acceptance uses a separate debug-only cfg, not a feature; run its
[isolated suite and release-rejection check](hardware-acceptance.md).
